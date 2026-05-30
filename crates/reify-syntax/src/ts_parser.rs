//! Tree-sitter based parser for the Reify language.
//!
//! Parses source text into tree-sitter CST, then lowers to the `ParsedModule` AST.

use std::cell::RefCell;
use std::collections::HashSet;

use reify_ast::*;
use reify_core::{ContentHash, ModulePath, PortDirection, SourceSpan, SpannedIdent};

/// Check a child node for errors before lowering it. If the node has errors,
/// push a parse error and return None. Otherwise, evaluate the lowering expression.
macro_rules! check_and_lower {
    ($self:ident, $child:ident, $label:expr, $lower:expr) => {
        if $child.is_error() || $child.has_error() {
            $self.push_error(
                format!("invalid {}: {}", $label, $self.node_text($child)),
                $self.span($child),
            );
            None
        } else {
            $lower
        }
    };
}

/// Parse source text into a `ParsedModule` using tree-sitter.
///
/// Equivalent to [`parse_with_prelude_enums(source, module_path, &[])`](parse_with_prelude_enums).
/// Use this entry when no prelude-supplied enum names need to participate in
/// the `EnumAccess` disambiguation pass — i.e. the source is self-contained
/// or will be compiled without a prelude.
pub fn parse(source: &str, module_path: ModulePath) -> ParsedModule {
    parse_with_prelude_enums(source, module_path, &[])
}

/// Parse source text into a `ParsedModule`, pre-seeding the lowering's
/// `known_enums` set with the supplied prelude enum names.
///
/// The disambiguation in `lower_member_access` resolves `Type.Variant` to
/// `EnumAccess` when `Type` is in `known_enums`, otherwise to `MemberAccess`.
/// Pre-seeding from a prelude lets the parser recognise stdlib/prelude enums
/// (e.g. `CorrosionClass.C5`) as `EnumAccess` even though their declarations
/// live outside the current source file.
///
/// `prelude_enum_names` and the source's own `enum_declaration` nodes are
/// merged into a single set; overlap between them is silently deduplicated by
/// `HashSet::insert` and emits no parse error — the parser does not police
/// name-resolution shadowing.  Compiler-side resolution decides which of the
/// two definitions wins.
///
/// Companion to [`reify_compiler::parse_with_stdlib`], which flattens the
/// stdlib's prelude enum names and delegates to this entry.
pub fn parse_with_prelude_enums(
    source: &str,
    module_path: ModulePath,
    prelude_enum_names: &[&'static str],
) -> ParsedModule {
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");

    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    let mut lowering = Lowering::with_prelude_enums(source, prelude_enum_names);
    lowering.lower_source_file(root);

    let content_hash = ContentHash::of_str(source);

    ParsedModule {
        path: module_path,
        declarations: lowering.declarations,
        errors: lowering.errors.into_inner(),
        content_hash,
        pragmas: lowering.module_pragmas,
    }
}

// ── Tree-walk helpers ────────────────────────────────────────────────────────

/// Walk `node`'s descendants depth-first and return the first node whose
/// `is_error()` or `is_missing()` is true, pruning subtrees where
/// `has_error()` is false for O(1) early-out on clean branches.
///
/// Uses an iterative `TreeCursor` pre-order walk (goto_first_child /
/// goto_next_sibling / goto_parent) rather than recursion, so deeply-nested
/// type-arg trees cannot cause a stack overflow — matching the iterative
/// tree-walk pattern used elsewhere in this file.
///
/// Uses the same `is_error() || is_missing()` predicate as the test-only
/// `count_errors` helper (ts_parser.rs test module) and the production guards
/// in struct/connect lowering — keeping the predicate shape canonical.
///
/// Returns `None` only when the subtree contains no ERROR or MISSING node.
/// Under the `has_error()` precondition at its sole call site this cannot
/// happen, so `None` is a purely defensive fallback.
fn first_error_or_missing_descendant(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    if !node.has_error() {
        return None; // O(1) prune — no error anywhere in this subtree
    }
    // Iterative pre-order DFS: descend into subtrees that contain an error,
    // skip clean subtrees in O(1), and terminate when we ascend back to `node`.
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return None; // defensive: has_error() true but node has no children
    }
    loop {
        let cur = cursor.node();
        if cur.is_error() || cur.is_missing() {
            return Some(cur);
        }
        // Descend only into subtrees that contain an error (O(1) per node).
        if cur.has_error() && cursor.goto_first_child() {
            continue;
        }
        // No error in this subtree (or no children); advance to next sibling,
        // ascending as needed until we find one or return to the starting node.
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() || cursor.node() == node {
                return None;
            }
        }
    }
}

/// CST → AST lowering context.
struct Lowering<'a> {
    source: &'a str,
    declarations: Vec<Declaration>,
    /// Interior mutability so that `&self` expression-lowering methods can emit diagnostics.
    errors: RefCell<Vec<ParseError>>,
    /// Enum names collected in the first pass for disambiguation.
    known_enums: HashSet<&'a str>,
    /// Module-level pragmas collected during source-file lowering.
    module_pragmas: Vec<Pragma>,
}

impl<'a> Lowering<'a> {
    /// Test-only constructor — equivalent to `with_prelude_enums(source, &[])`.
    /// Production callers go through `parse` / `parse_with_prelude_enums`,
    /// which use `with_prelude_enums` directly.
    #[cfg(test)]
    fn new(source: &'a str) -> Self {
        Self::with_prelude_enums(source, &[])
    }

    /// Construct a lowering context whose `known_enums` set is pre-seeded
    /// with `prelude_enum_names`.  The first-pass collector in
    /// `lower_source_file` then unions the current source's own enum names
    /// into the same set.  `HashSet::insert` deduplicates any overlap
    /// silently — see `parse_with_prelude_enums` for the full contract.
    fn with_prelude_enums(source: &'a str, prelude_enum_names: &[&'static str]) -> Self {
        let mut known_enums: HashSet<&'a str> = HashSet::new();
        for &name in prelude_enum_names {
            known_enums.insert(name);
        }
        Self {
            source,
            declarations: Vec::new(),
            errors: RefCell::new(Vec::new()),
            known_enums,
            module_pragmas: Vec::new(),
        }
    }

    /// Push a parse error diagnostic.
    fn push_error(&self, message: String, span: SourceSpan) {
        self.errors.borrow_mut().push(ParseError { message, span });
    }

    /// Extract the source text for a node.
    fn node_text(&self, node: tree_sitter::Node) -> &'a str {
        &self.source[node.start_byte()..node.end_byte()]
    }

    /// Create a SourceSpan from a tree-sitter node.
    fn span(&self, node: tree_sitter::Node) -> SourceSpan {
        SourceSpan::new(node.start_byte() as u32, node.end_byte() as u32)
    }

    /// Compute content hash for a node from its source text.
    fn content_hash(&self, node: tree_sitter::Node) -> ContentHash {
        ContentHash::of_str(self.node_text(node))
    }

    /// Emit a diagnostic for an unexpected named child in a lowering context.
    ///
    /// Skips anonymous tokens and extras (comments). For named, non-extra
    /// children that don't match any expected arm, pushes an error with the
    /// child's kind and source text.
    fn warn_unexpected_child(&mut self, child: tree_sitter::Node, context: &str) {
        if child.is_named() && !child.is_extra() {
            self.push_error(
                format!(
                    "unexpected '{}' in {}: {}",
                    child.kind(),
                    context,
                    self.node_text(child)
                ),
                self.span(child),
            );
        }
    }

    /// Extract a doc comment from `///` line comments immediately preceding a node.
    ///
    /// Walks backward through previous siblings collecting consecutive `line_comment`
    /// nodes whose text starts with `///`. Returns `None` if no doc comments are found.
    fn extract_doc_comment(&self, node: tree_sitter::Node) -> Option<String> {
        let mut lines = Vec::new();
        let mut sibling = node.prev_sibling();
        while let Some(s) = sibling {
            if s.kind() == "line_comment" {
                let text = self.node_text(s);
                if let Some(stripped) = text.strip_prefix("///") {
                    // Collect in reverse order (we walk backward)
                    lines.push(stripped.strip_prefix(' ').unwrap_or(stripped));
                    sibling = s.prev_sibling();
                    continue;
                }
            }
            break;
        }
        if lines.is_empty() {
            return None;
        }
        lines.reverse();
        Some(lines.join("\n"))
    }

    /// Check if a node has an anonymous 'pub' keyword child.
    fn has_pub_keyword(&self, node: tree_sitter::Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !child.is_named() && self.node_text(child) == "pub" {
                return true;
            }
        }
        false
    }

    /// Check if a node has an anonymous 'aux' keyword child.
    ///
    /// Mirrors `has_pub_keyword`. Used by `lower_let` and `lower_sub` to set
    /// `is_aux` (PRD §2.1/§2.2, task 3899 step-6).
    fn has_aux_keyword(&self, node: tree_sitter::Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !child.is_named() && self.node_text(child) == "aux" {
                return true;
            }
        }
        false
    }

    // ── Top-level lowering ──────────────────────────────────

    fn lower_source_file(&mut self, node: tree_sitter::Node) {
        // First pass: collect enum names for disambiguation of member_access
        // vs EnumAccess in expressions. This enables order-independent declarations.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_declaration"
                && let Some(name_node) = child.child_by_field_name("name")
            {
                self.known_enums.insert(self.node_text(name_node));
            }
        }

        // Second pass: lower all declarations.
        // Annotations immediately before a declaration are accumulated in
        // `pending_annotations` and drained into the declaration's `annotations` field.
        let mut pending_annotations: Vec<Annotation> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "structure_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_structure(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Structure(decl));
                    }
                }
                "occurrence_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_occurrence(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Occurrence(decl));
                    }
                }
                "import_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_import(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Import(decl));
                    }
                }
                "enum_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_enum(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Enum(decl));
                    }
                }
                "function_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_function(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Function(decl));
                    }
                }
                "trait_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_trait(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Trait(decl));
                    }
                }
                "field_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_field(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Field(decl));
                    }
                }
                "purpose_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_purpose(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Purpose(decl));
                    }
                }
                "constraint_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_constraint_def(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Constraint(decl));
                    }
                }
                "unit_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_unit(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Unit(decl));
                    }
                }
                "type_alias_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut decl) = self.lower_type_alias(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::TypeAlias(decl));
                    }
                }
                "annotation" => {
                    if let Some(annotation) = self.lower_annotation(child) {
                        pending_annotations.push(annotation);
                    }
                }
                "pragma" => {
                    if let Some(pragma) = self.lower_pragma(child) {
                        self.module_pragmas.push(pragma);
                    }
                }
                "ERROR" => {
                    // Consume any pending annotations so they don't leak past a
                    // syntax error to the next successfully-parsed declaration.
                    let _ = std::mem::take(&mut pending_annotations);
                    self.push_error(
                        format!("syntax error: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => self.warn_unexpected_child(child, "source file"),
            }
        }
    }

    fn lower_import(&self, node: tree_sitter::Node) -> Option<ImportDecl> {
        let is_pub = self.has_pub_keyword(node);

        // Extract the dot-separated path segments from import_path node
        let path_node = node.child_by_field_name("path")?;
        let mut segments = Vec::new();
        let mut cursor = path_node.walk();
        for child in path_node.children(&mut cursor) {
            if child.kind() == "identifier" {
                segments.push(self.node_text(child).to_string());
            }
        }

        // Determine the ImportKind based on optional suffix nodes
        let items_node = node.child_by_field_name("items");
        let alias_node = node.child_by_field_name("alias");

        let (path, kind) = if let Some(items) = items_node {
            // Destructured: `import a.b.{C, D}`
            let path = segments.join(".");
            let mut names = Vec::new();
            let mut items_cursor = items.walk();
            for child in items.children(&mut items_cursor) {
                if child.kind() == "identifier" {
                    names.push(self.node_text(child).to_string());
                }
            }
            (path, ImportKind::Destructured(names))
        } else if let Some(alias) = alias_node {
            let alias_name = self.node_text(alias).to_string();
            // Check if the last segment looks like an entity (starts with uppercase)
            if segments.len() >= 2
                && segments
                    .last()
                    .is_some_and(|s| s.starts_with(|c: char| c.is_uppercase()))
            {
                // EntityAliased: `import a.b.Entity as Alias`
                let entity = segments.pop().unwrap();
                let path = segments.join(".");
                (
                    path,
                    ImportKind::EntityAliased {
                        entity,
                        alias: alias_name,
                    },
                )
            } else {
                // Aliased: `import a.b as x`
                let path = segments.join(".");
                (path, ImportKind::Aliased { alias: alias_name })
            }
        } else {
            // No items, no alias — check if last segment is an entity (uppercase)
            if segments.len() >= 2
                && segments
                    .last()
                    .is_some_and(|s| s.starts_with(|c: char| c.is_uppercase()))
            {
                // Entity: `import a.b.Entity`
                let entity = segments.pop().unwrap();
                let path = segments.join(".");
                (path, ImportKind::Entity(entity))
            } else {
                // Module: `import a.b`
                let path = segments.join(".");
                (path, ImportKind::Module)
            }
        };

        Some(ImportDecl {
            path,
            kind,
            is_pub,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    fn lower_enum(&self, node: tree_sitter::Node) -> Option<EnumDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        // Detect 'pub' keyword by checking anonymous children
        let is_pub = self.has_pub_keyword(node);

        // Collect variant identifiers — skip 'enum', name, '{', '}', ','
        let mut variants = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" && child.id() != name_node.id() {
                variants.push(self.node_text(child).to_string());
            }
        }

        let doc = self.extract_doc_comment(node);

        Some(EnumDecl {
            name,
            doc,
            is_pub,
            variants,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    /// Extract identifiers from a trait_bound_list node (e.g., `Rigid + Printable`).
    fn lower_trait_bound_list(&self, node: tree_sitter::Node) -> Vec<String> {
        let mut bounds = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "trait_bound_entry" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    bounds.push(self.node_text(name_node).to_string());
                }
            } else if child.kind() == "identifier" {
                bounds.push(self.node_text(child).to_string());
            }
        }
        bounds
    }

    /// Extract type parameters from a node's optional type_parameters child.
    fn lower_type_parameters(&self, node: tree_sitter::Node) -> Vec<TypeParamDecl> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_parameters" {
                return self.lower_type_params_inner(child);
            }
        }
        vec![]
    }

    /// Lower the contents of a type_parameters node.
    fn lower_type_params_inner(&self, node: tree_sitter::Node) -> Vec<TypeParamDecl> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "type_parameter"
                && let Some(name_node) = child.child_by_field_name("name")
            {
                let name = self.node_text(name_node).to_string();
                let bounds = child
                    .child_by_field_name("bounds")
                    .map(|b| self.lower_trait_bound_list(b))
                    .unwrap_or_default();
                let default = child
                    .child_by_field_name("default")
                    .map(|d| self.lower_type_expr_node(d));
                params.push(TypeParamDecl {
                    name,
                    bounds,
                    default,
                    span: self.span(child),
                });
            }
        }
        params
    }

    /// Find a trait_bound_list child and extract full TraitBoundRef entries.
    fn find_trait_bound_refs(&self, node: tree_sitter::Node) -> Vec<TraitBoundRef> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "trait_bound_list" {
                return self.lower_trait_bound_refs(child);
            }
        }
        vec![]
    }

    /// Extract TraitBoundRef entries from a trait_bound_list node.
    fn lower_trait_bound_refs(&self, node: tree_sitter::Node) -> Vec<TraitBoundRef> {
        let mut bounds = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "trait_bound_entry"
                && let Some(name_node) = child.child_by_field_name("name")
            {
                let type_args = self.lower_type_args_from_node(child);
                bounds.push(TraitBoundRef {
                    name: self.node_text(name_node).to_string(),
                    type_args,
                    span: self.span(child),
                });
            }
        }
        bounds
    }

    /// Find a trait_bound_list child and extract refinement entries as [`SpannedIdent`] values.
    ///
    /// Delegates to [`find_trait_bound_refs`] and projects each [`TraitBoundRef`] to a
    /// [`SpannedIdent`] (dropping the unused `type_args`). This keeps the walking logic in one
    /// place so grammar changes to `trait_bound_entry` shapes only need to be handled once.
    fn find_trait_refinement_list(&self, node: tree_sitter::Node) -> Vec<SpannedIdent> {
        self.find_trait_bound_refs(node)
            .into_iter()
            .map(|tbr| SpannedIdent {
                name: tbr.name,
                span: tbr.span,
            })
            .collect()
    }

    /// Lower a type_expr node to a TypeExpr. Handles both bare identifiers and parameterized types.
    fn lower_type_expr_node(&self, node: tree_sitter::Node) -> TypeExpr {
        if node.kind() == "type_expr" {
            // type_expr is choice(parameterized_type, identifier)
            let child = node.child(0).unwrap_or(node);
            if child.kind() == "parameterized_type" {
                return self.lower_parameterized_type(child);
            }
            // bare identifier
            TypeExpr {
                kind: TypeExprKind::Named {
                    name: self.node_text(child).to_string(),
                    type_args: vec![],
                },
                span: self.span(child),
            }
        } else if node.kind() == "parameterized_type" {
            self.lower_parameterized_type(node)
        } else {
            // treat as bare identifier
            TypeExpr {
                kind: TypeExprKind::Named {
                    name: self.node_text(node).to_string(),
                    type_args: vec![],
                },
                span: self.span(node),
            }
        }
    }

    /// Lower a parameterized_type node (e.g., Box<T>) to a TypeExpr.
    fn lower_parameterized_type(&self, node: tree_sitter::Node) -> TypeExpr {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n).to_string())
            .unwrap_or_default();
        let type_args = self.lower_type_args_from_node(node);
        TypeExpr {
            kind: TypeExprKind::Named { name, type_args },
            span: self.span(node),
        }
    }

    /// Extract type arguments from a node that has a type_args field or type_arg_list child.
    ///
    /// Type-arg-list elements come in two shapes:
    /// - A `type_expr` / `parameterized_type` / `identifier` node, lowered to
    ///   `TypeExprKind::Named` (or a deeper structure) via `lower_type_expr_node`.
    /// - A `number_literal` node, used by parametric `Tensor<r,n,q>` and
    ///   `Matrix<m,n,q>` syntax. Lowered to `TypeExprKind::IntegerLiteral`.
    ///   Non-integer literals (e.g. `Tensor<2.5, ...>`) are recorded with the
    ///   integer part dropped — type resolution issues a diagnostic when this
    ///   variant appears in a non-Tensor/Matrix slot or when the literal is
    ///   non-integral.
    fn lower_type_args_from_node(&self, node: tree_sitter::Node) -> Vec<TypeExpr> {
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "type_arg_list" {
                // AC#1: recursively scan the type_arg_list subtree for any ERROR
                // or MISSING node (tree-sitter's has_error() does this in O(1)).
                // Mirrors the "ERROR" => arm in lower_source_file (ts_parser.rs:305-313).
                // Emit exactly ONE aggregated diagnostic per malformed type_arg_list to
                // avoid per-ERROR-node spam when recovery produces multiple fragments.
                // Do NOT early-return: well-formed siblings of the error node are still
                // lowered so callers see a partial AST instead of an empty type_args list.
                // ERROR-bearing children naturally fail to match any inner kind branch and
                // are skipped; only the aggregated diagnostic is emitted.
                //
                // Task 3725: narrow the diagnostic span to the first ERROR/MISSING
                // descendant so the span does not cover well-formed sibling arguments.
                // first_error_or_missing_descendant prunes clean subtrees in O(1) via
                // has_error(); the fallback to self.span(child) is purely defensive —
                // has_error() guarantees at least one ERROR/MISSING exists.
                if child.has_error() {
                    let fault_span = first_error_or_missing_descendant(child)
                        .map(|n| self.span(n))
                        .unwrap_or_else(|| self.span(child));
                    self.push_error(
                        "syntax error in type argument list".to_string(),
                        fault_span,
                    );
                }
                let mut inner_cursor = child.walk();
                for inner in child.named_children(&mut inner_cursor) {
                    if inner.kind() == "type_expr"
                        || inner.kind() == "parameterized_type"
                        || inner.kind() == "identifier"
                    {
                        args.push(self.lower_type_expr_node(inner));
                    } else if inner.kind() == "number_literal" {
                        let text = self.node_text(inner);
                        // Parse as u32. Float literals (e.g. "2.5") fail to_parse and lower to 0;
                        // type-resolution surfaces a diagnostic for non-integer / out-of-range
                        // type arguments.
                        let value: u32 = text.parse().unwrap_or(0);
                        args.push(TypeExpr {
                            kind: TypeExprKind::IntegerLiteral(value),
                            span: self.span(inner),
                        });
                    } else if inner.kind() == "auto_type_arg" {
                        // Locate the auto_keyword child to check for the free modifier.
                        // Reuses the same child_by_field_name("modifier").is_some() pattern as
                        // lower_param (ts_parser.rs:1582-1592) — auto_keyword is shared between
                        // param-default and type-arg positions (grammar.js:433-436, 654-657).
                        let mut kw_cursor = inner.walk();
                        let kw_opt = inner
                            .named_children(&mut kw_cursor)
                            .find(|n| n.kind() == "auto_keyword");
                        // Grammar invariant (grammar.js:663-667): tree-sitter-reify always
                        // inserts a MISSING `auto_keyword` child for malformed `auto_type_arg`
                        // nodes (verified by a 15-input CST probe; task 3724), so kw_opt is
                        // always Some under any currently-known input.  The push_error else-arm
                        // is defense-in-depth, mirroring the sibling bound-missing guard
                        // (lines 704-710): if a future grammar change ever weakens the
                        // MISSING-node invariant, release users see the diagnostic instead of
                        // a silently-dropped AST entry.
                        let Some(kw) = kw_opt else {
                            self.push_error(
                                "auto type-arg missing auto keyword".to_string(),
                                self.span(inner),
                            );
                            continue;
                        };
                        let free = kw.child_by_field_name("modifier").is_some();
                        // The grammar guarantees a `bound` field (bare identifier) on every
                        // well-formed auto_type_arg. Guard defensively: if error recovery
                        // produces an auto_type_arg without a bound, emit a diagnostic and
                        // skip the entry rather than propagating an empty string into the
                        // AST (which would corrupt Display output and collect_type_expr_names).
                        let Some(bound_node) = inner.child_by_field_name("bound") else {
                            self.push_error(
                                "auto type-arg missing bound identifier".to_string(),
                                self.span(inner),
                            );
                            continue;
                        };
                        let bound = self.node_text(bound_node).to_string();
                        args.push(TypeExpr {
                            kind: TypeExprKind::Auto { free, bound },
                            span: self.span(inner),
                        });
                    }
                }
                return args;
            }
        }
        args
    }

    // ── Function lowering ─────────────────────────────────────

    fn lower_function(&self, node: tree_sitter::Node) -> Option<FnDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);
        let is_pub = self.has_pub_keyword(node);

        // Extract optional type parameters
        let type_params = self.lower_type_parameters(node);

        // Extract function params from fn_param_list.
        //
        // When `fn_param_list` has a `receiver` field (the `self` keyword),
        // prepend a synthetic FnParam with `is_self = true` and a sentinel
        // `TypeExprKind::Named { name: "self" }` type (placeholder, replaced by
        // the concrete receiver type during dispatch in task δ/ζ).  Typed params
        // that follow `self` are lowered as normal (is_self = false).
        //
        // Top-level `Declaration::Function` never has a receiver field; only
        // trait-member `function_definition`/`function_signature` nodes do.
        let params = {
            let mut cursor = node.walk();
            let mut params = Vec::new();
            for child in node.children(&mut cursor) {
                if child.kind() == "fn_param_list" {
                    // Check for a `self` receiver field.
                    if let Some(receiver_node) = child.child_by_field_name("receiver") {
                        let receiver_span = self.span(receiver_node);
                        params.push(FnParam {
                            name: "self".to_string(),
                            is_self: true,
                            type_expr: TypeExpr {
                                kind: TypeExprKind::Named {
                                    name: "self".to_string(),
                                    type_args: vec![],
                                },
                                span: receiver_span,
                            },
                            default: None,
                            span: receiver_span,
                        });
                    }
                    // Collect typed fn_param children (is_self = false via lower_fn_param).
                    let mut param_cursor = child.walk();
                    for param_child in child.children(&mut param_cursor) {
                        if param_child.kind() == "fn_param"
                            && let Some(p) = self.lower_fn_param(param_child)
                        {
                            params.push(p);
                        }
                    }
                    break;
                }
            }
            params
        };

        // Extract optional return type
        let return_type = node
            .child_by_field_name("return_type")
            .map(|t| self.lower_type_expr_node(t));

        // Extract fn_body — `Some` for function_definition (has a body block),
        // `None` for function_signature (bodyless required trait fn).
        let body = {
            let mut cursor = node.walk();
            let mut body = None;
            for child in node.children(&mut cursor) {
                if child.kind() == "fn_body" {
                    body = self.lower_fn_body(child);
                    break;
                }
            }
            body
        };

        Some(FnDef {
            name,
            doc,
            is_pub,
            type_params,
            params,
            return_type,
            body,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    // ── Trait lowering ────────────────────────────────────────

    fn lower_trait(&mut self, node: tree_sitter::Node) -> Option<TraitDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);
        let is_pub = self.has_pub_keyword(node);
        let type_params = self.lower_type_parameters(node);

        // Extract refinements from optional trait_bound_list child;
        // each entry carries its precise byte-offset span for diagnostics.
        let refinements = self.find_trait_refinement_list(node);

        let (members, pragmas) = self.lower_trait_members(node);

        Some(TraitDecl {
            name,
            doc,
            is_pub,
            type_params,
            refinements,
            members,
            span: self.span(node),
            content_hash: self.content_hash(node),
            pragmas,
            annotations: vec![],
        })
    }

    fn lower_field(&mut self, node: tree_sitter::Node) -> Option<FieldDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();
        let is_pub = self.has_pub_keyword(node);

        let domain_node = node.child_by_field_name("domain")?;
        let domain_type = self.lower_type_expr_node(domain_node);

        let codomain_node = node.child_by_field_name("codomain")?;
        let codomain_type = self.lower_type_expr_node(codomain_node);

        let source_node = node.child_by_field_name("source")?;
        let source = self.lower_field_source(source_node)?;

        Some(FieldDef {
            name,
            is_pub,
            domain_type,
            codomain_type,
            source,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    fn lower_field_source(&mut self, node: tree_sitter::Node) -> Option<FieldSource> {
        // field_source is a choice node; get its first named child
        let inner = node.named_child(0)?;
        match inner.kind() {
            "field_source_analytical" => {
                let expr_node = inner.child_by_field_name("expr")?;
                let expr = self.lower_expr(expr_node)?;
                Some(FieldSource::Analytical { expr })
            }
            "field_source_sampled" => {
                let mut config = Vec::new();
                let mut cursor = inner.walk();
                for child in inner.named_children(&mut cursor) {
                    if child.kind() == "field_config_entry"
                        && let Some(key_node) = child.child_by_field_name("key")
                    {
                        let key = self.node_text(key_node).to_string();
                        if let Some(val_node) = child.child_by_field_name("value")
                            && let Some(val_expr) = self.lower_expr(val_node)
                        {
                            config.push((key, val_expr));
                        }
                    }
                }
                Some(FieldSource::Sampled { config })
            }
            "field_source_composed" => {
                let expr_node = inner.child_by_field_name("expr")?;
                let expr = self.lower_expr(expr_node)?;
                Some(FieldSource::Composed { expr })
            }
            "field_source_imported" => {
                let mut path: Option<String> = None;
                let mut format: Option<String> = None;
                let mut grid: Option<String> = None;
                let mut cursor = inner.walk();
                for child in inner.named_children(&mut cursor) {
                    if child.kind() == "field_config_entry"
                        && let Some(key_node) = child.child_by_field_name("key")
                    {
                        let key = self.node_text(key_node).to_string();
                        if let Some(val_node) = child.child_by_field_name("value")
                            && let Some(val_expr) = self.lower_expr(val_node)
                        {
                            match key.as_str() {
                                "path" => {
                                    if let ExprKind::StringLiteral(s) = val_expr.kind {
                                        path = Some(s);
                                    }
                                }
                                "format" => {
                                    if let ExprKind::Ident(s) = val_expr.kind {
                                        format = Some(s);
                                    }
                                }
                                "grid" => {
                                    if let ExprKind::StringLiteral(s) = val_expr.kind {
                                        grid = Some(s);
                                    }
                                }
                                _ => {
                                    // Unknown keys are silently dropped here; the AST
                                    // has no extras field, so they are unrecoverable at
                                    // compile time. This is intentional: the open grammar
                                    // provides forward-compatibility (v0.3 keys won't
                                    // cause parse errors), while compile-phase diagnostics
                                    // are limited to the three known fields.
                                    //
                                    // Note: the same applies to known keys whose value
                                    // expression kind doesn't match expectations (e.g.
                                    // `path = OpenVDB` instead of a string literal) — the
                                    // field stays None and the compiler diagnoses
                                    // "missing path" rather than "path has wrong type".
                                }
                            }
                        }
                    }
                }
                Some(FieldSource::Imported { path, format, grid })
            }
            _ => None,
        }
    }

    fn lower_purpose(&mut self, node: tree_sitter::Node) -> Option<PurposeDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let is_pub = self.has_pub_keyword(node);
        let type_params = self.lower_type_parameters(node);
        let params = self.lower_purpose_params(node);
        let (members, pragmas) = self.lower_purpose_members(node);

        Some(PurposeDef {
            name,
            is_pub,
            type_params,
            params,
            members,
            span: self.span(node),
            content_hash: self.content_hash(node),
            pragmas,
            annotations: vec![],
        })
    }

    // ── Constraint definition lowering ───────────────────────────

    fn lower_constraint_def(&mut self, node: tree_sitter::Node) -> Option<ConstraintDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let is_pub = self.has_pub_keyword(node);
        let type_params = self.lower_type_parameters(node);

        let mut params = Vec::new();
        let mut predicates = Vec::new();
        let mut pragmas = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "param_declaration" => {
                    let _ = check_and_lower!(
                        self,
                        child,
                        "constraint param",
                        self.lower_param(child).map(|p| params.push(p))
                    );
                }
                "let_declaration" => {
                    // let declarations in constraint def body are ignored for now
                    // (captured in params/predicates separation; future: add lets field)
                }
                "constraint_def_predicate" => {
                    if let Some(expr_node) = child.child_by_field_name("expr")
                        && let Some(expr) = self.lower_expr(expr_node)
                    {
                        predicates.push(expr);
                    }
                }
                "pragma" => {
                    if let Some(pragma) = self.lower_pragma(child) {
                        pragmas.push(pragma);
                    }
                }
                // identifier (name) and type_parameters are already handled
                // before the loop via child_by_field_name / lower_type_parameters.
                "identifier" | "type_parameters" => {}
                "ERROR" => {
                    self.push_error(
                        format!("syntax error in constraint body: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => self.warn_unexpected_child(child, "constraint body"),
            }
        }

        Some(ConstraintDef {
            name,
            is_pub,
            type_params,
            params,
            predicates,
            span: self.span(node),
            content_hash: self.content_hash(node),
            pragmas,
            annotations: vec![],
        })
    }

    fn lower_unit(&mut self, node: tree_sitter::Node) -> Option<UnitDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let is_pub = self.has_pub_keyword(node);

        let type_node = node.child_by_field_name("type")?;
        let dimension_type = self.lower_type_expr_node(type_node);

        let conversion = node
            .child_by_field_name("conversion")
            .and_then(|n| self.lower_expr(n));

        let offset = node
            .child_by_field_name("offset")
            .and_then(|n| self.lower_expr(n));

        Some(UnitDecl {
            name,
            is_pub,
            dimension_type,
            conversion,
            offset,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    fn lower_type_alias(&mut self, node: tree_sitter::Node) -> Option<TypeAliasDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);
        let is_pub = self.has_pub_keyword(node);
        let type_params = self.lower_type_parameters(node);

        let type_node = node.child_by_field_name("type")?;
        let type_expr = self.lower_dimensional_type_expr(type_node);

        Some(TypeAliasDecl {
            name,
            doc,
            is_pub,
            type_params,
            type_expr,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    /// Lower a dimensional_type_expr node. Handles binary operations on types
    /// (e.g., `Force / Area`, `Mass * Length`) and delegates to `lower_type_expr_node`
    /// for leaf type expressions.
    fn lower_dimensional_type_expr(&mut self, node: tree_sitter::Node) -> TypeExpr {
        if node.kind() == "dimensional_type_expr" {
            // Check if this is a binary op (has op field) or a passthrough to type_expr
            if let Some(op_node) = node.child_by_field_name("op") {
                let op = self.node_text(op_node).to_string();
                let left_node = match node.child_by_field_name("left") {
                    Some(n) if !n.is_missing() && !n.is_error() && !n.has_error() => n,
                    _ => {
                        self.push_error(
                            "dimensional type expression missing left operand".to_string(),
                            self.span(node),
                        );
                        return self.lower_type_expr_node(node);
                    }
                };
                let right_node = match node.child_by_field_name("right") {
                    Some(n) if !n.is_missing() && !n.is_error() && !n.has_error() => n,
                    _ => {
                        self.push_error(
                            "dimensional type expression missing right operand".to_string(),
                            self.span(node),
                        );
                        return self.lower_type_expr_node(node);
                    }
                };
                let left = self.lower_dimensional_type_expr(left_node);
                let right = self.lower_dimensional_type_expr(right_node);
                let dim_op = if op == "*" { DimOp::Mul } else { DimOp::Div };
                return TypeExpr {
                    kind: TypeExprKind::DimensionalOp {
                        op: dim_op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span: self.span(node),
                };
            }
            // Passthrough: dimensional_type_expr -> type_expr
            let child = node.child(0).unwrap_or(node);
            return self.lower_type_expr_node(child);
        }
        // Fallback: treat as a regular type expression
        self.lower_type_expr_node(node)
    }

    // ── Annotation lowering ───────────────────────────────────

    /// Lower an `annotation` CST node to an `Annotation` AST node.
    ///
    /// Grammar: `'@' name:immediate_identifier ('(' commaSep(_expression) ')')?`
    fn lower_annotation(&self, node: tree_sitter::Node) -> Option<Annotation> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        // Collect expression args from named children (skipping the name field itself).
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.id() != name_node.id()
                && let Some(expr) = self.lower_expr(child)
            {
                args.push(expr);
            }
        }

        Some(Annotation {
            name,
            args,
            span: self.span(node),
        })
    }

    // ── Pragma lowering ───────────────────────────────────────

    /// Lower a `pragma` CST node to a `Pragma` AST node.
    ///
    /// Grammar: `'#' name:immediate_identifier ('(' commaSep(pragma_arg) ')')?`
    fn lower_pragma(&self, node: tree_sitter::Node) -> Option<Pragma> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        // Collect args from pragma_arg children (if any).
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "pragma_arg"
                && let Some(arg) = self.lower_pragma_arg(child)
            {
                args.push(arg);
            }
        }

        Some(Pragma {
            name,
            args,
            span: self.span(node),
        })
    }

    /// Lower a `pragma_arg` CST node.
    ///
    /// Grammar: `(key:identifier '=' value:_pragma_value) | value:_pragma_value`
    fn lower_pragma_arg(&self, node: tree_sitter::Node) -> Option<PragmaArg> {
        if let Some(key_node) = node.child_by_field_name("key") {
            // KeyValue form: `key = value`
            let key = self.node_text(key_node).to_string();
            let value_node = node.child_by_field_name("value")?;
            let value = self.lower_pragma_value(value_node)?;
            Some(PragmaArg::KeyValue { key, value })
        } else if let Some(value_node) = node.child_by_field_name("value") {
            // Bare form: just a value
            let value = self.lower_pragma_value(value_node)?;
            Some(PragmaArg::Bare(value))
        } else {
            None
        }
    }

    /// Lower a `_pragma_value` CST node to a `PragmaValue`.
    fn lower_pragma_value(&self, node: tree_sitter::Node) -> Option<PragmaValue> {
        match node.kind() {
            "identifier" => Some(PragmaValue::Ident(self.node_text(node).to_string())),
            "number_literal" => {
                let text = self.node_text(node);
                Self::strip_underscores_and_parse(text).map(PragmaValue::Number)
            }
            "quantity_literal" => {
                let value_node = node.child_by_field_name("value")?;
                let unit_node = node.child_by_field_name("unit")?;
                let value: f64 = Self::strip_underscores_and_parse(self.node_text(value_node))?;
                let unit = self.node_text(unit_node).to_string();
                Some(PragmaValue::Quantity { value, unit })
            }
            "string_literal" => {
                let raw = self.node_text(node);
                // Strip the surrounding quotes.
                let s = raw
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(raw)
                    .to_string();
                Some(PragmaValue::String(s))
            }
            "bool_literal" => match self.node_text(node) {
                "true" => Some(PragmaValue::Bool(true)),
                "false" => Some(PragmaValue::Bool(false)),
                _ => None,
            },
            _ => None,
        }
    }

    fn lower_purpose_params(&self, node: tree_sitter::Node) -> Vec<PurposeParam> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "purpose_param"
                && let Some(param) = self.lower_purpose_param(child)
            {
                params.push(param);
            }
        }
        params
    }

    fn lower_purpose_param(&self, node: tree_sitter::Node) -> Option<PurposeParam> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let kind_node = node.child_by_field_name("entity_kind")?;
        let entity_kind = self.node_text(kind_node).to_string();

        Some(PurposeParam {
            name,
            entity_kind,
            span: self.span(node),
        })
    }

    fn lower_purpose_members(&mut self, node: tree_sitter::Node) -> (Vec<MemberDecl>, Vec<Pragma>) {
        let mut members = Vec::new();
        let mut pragmas = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "purpose_member" {
                // purpose_member is a choice node wrapping the actual member or pragma
                if let Some(inner) = child.named_child(0) {
                    if inner.kind() == "pragma" {
                        if let Some(pragma) = self.lower_pragma(inner) {
                            pragmas.push(pragma);
                        }
                    } else if let Some(member) = self.lower_member(inner) {
                        members.push(member);
                    }
                }
            }
        }
        (members, pragmas)
    }

    fn lower_fn_param(&self, node: tree_sitter::Node) -> Option<FnParam> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_node = node.child_by_field_name("type")?;
        let type_expr = self.lower_type_expr_node(type_node);

        // Note: lower_fn_param diagnoses unrecognised defaults (user-facing error);
        // lower_param silently drops via .and_then — see lower_param below for rationale.
        let default = if let Some(d) = node.child_by_field_name("default") {
            if let Some(expr) = self.lower_expr(d) {
                Some(expr)
            } else {
                // Defensive branch: grammar.js:83-88 binds fn_param.default to
                // $._expression, and lower_expr exhaustively matches every
                // _expression kind (see ts_parser.rs ~line 2162), so this arm is
                // unreachable from a well-formed CST. It is only reachable via
                // error-recovery partial/ERROR nodes, which already set has_error().
                // The diagnostic is retained as defense-in-depth so a malformed
                // default surfaces a message rather than silently becoming "no default".
                self.push_error(
                    format!(
                        "unrecognised expression in fn_param default: {}",
                        self.node_text(d)
                    ),
                    self.span(d),
                );
                None
            }
        } else {
            None
        };

        Some(FnParam {
            name,
            is_self: false,
            type_expr,
            default,
            span: self.span(node),
        })
    }

    fn lower_fn_body(&self, node: tree_sitter::Node) -> Option<FnBody> {
        // Desugar contract (task 3919, spec §18 #10):
        //
        // `fn_body` has two grammar arms:
        //   block form:      `{ [fn_let_binding*]  result:<expr> }`
        //   expression form: `= result:<expr>`
        //
        // Both arms share the `result` field name.  This function therefore
        // handles both arms uniformly:
        //   - Block form: collects fn_let_binding children (may be empty), then
        //     reads `result`.  Yields FnBody { let_bindings, result_expr }.
        //   - Expression form: the loop below finds zero fn_let_binding children
        //     (there are none), so let_bindings = vec![].  `child_by_field_name("result")`
        //     resolves the `= expr` arm's result field identically.
        //     Yields FnBody { let_bindings: vec![], result_expr } — structurally
        //     identical to a block body with no let bindings.  Pure desugar.
        //
        // No branching on grammar arm is required.
        let mut let_bindings = Vec::new();

        // Collect fn_let_binding children (zero for the expression form).
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "fn_let_binding"
                && let Some(let_decl) = self.lower_fn_let_binding(child)
            {
                let_bindings.push(let_decl);
            }
        }

        // The result expression is the 'result' field — present in both arms.
        let result_node = node.child_by_field_name("result")?;
        let result_expr = self.lower_expr(result_node)?;

        Some(FnBody {
            let_bindings,
            result_expr,
        })
    }

    fn lower_fn_let_binding(&self, node: tree_sitter::Node) -> Option<LetDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_expr = node
            .child_by_field_name("type")
            .map(|t| self.lower_type_expr_node(t));

        let value_node = node.child_by_field_name("value")?;
        let value = self.lower_expr(value_node)?;

        Some(LetDecl {
            name,
            doc: None, // fn let bindings don't have doc comments
            type_expr,
            is_pub: false,
            is_aux: false,
            value,
            where_clause: None, // fn let bindings have no where clause
            annotations: Vec::new(),
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    /// Collect members and block-level pragmas from trait_member children of a trait_declaration node.
    fn lower_trait_members(&mut self, node: tree_sitter::Node) -> (Vec<MemberDecl>, Vec<Pragma>) {
        let mut members = Vec::new();
        let mut pragmas = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "trait_member" {
                // trait_member is a choice node wrapping the actual member or pragma
                if let Some(inner) = child.named_child(0) {
                    if inner.kind() == "pragma" {
                        if let Some(pragma) = self.lower_pragma(inner) {
                            pragmas.push(pragma);
                        }
                    } else if let Some(member) = self.lower_member(inner) {
                        members.push(member);
                    }
                }
            }
        }
        (members, pragmas)
    }

    fn lower_associated_type(&self, node: tree_sitter::Node) -> Option<AssociatedTypeDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let default_type = node
            .child_by_field_name("default")
            .map(|t| self.lower_type_expr_node(t));

        Some(AssociatedTypeDecl {
            name,
            default_type,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    /// Lower a single member node (used by both lower_structure and lower_guarded_block).
    fn lower_member(&mut self, child: tree_sitter::Node) -> Option<MemberDecl> {
        match child.kind() {
            "param_declaration" => check_and_lower!(
                self,
                child,
                "param",
                self.lower_param(child).map(MemberDecl::Param)
            ),
            "let_declaration" => check_and_lower!(
                self,
                child,
                "let",
                self.lower_let(child).map(MemberDecl::Let)
            ),
            "constraint_declaration" => check_and_lower!(
                self,
                child,
                "constraint",
                self.lower_constraint(child).map(MemberDecl::Constraint)
            ),
            "sub_declaration" => check_and_lower!(
                self,
                child,
                "sub",
                self.lower_sub(child).map(MemberDecl::Sub)
            ),
            "minimize_declaration" => check_and_lower!(
                self,
                child,
                "minimize",
                self.lower_minimize(child).map(MemberDecl::Minimize)
            ),
            "maximize_declaration" => check_and_lower!(
                self,
                child,
                "maximize",
                self.lower_maximize(child).map(MemberDecl::Maximize)
            ),
            "guarded_block" => check_and_lower!(
                self,
                child,
                "guarded block",
                self.lower_guarded_block(child)
            ),
            "associated_type" => self
                .lower_associated_type(child)
                .map(MemberDecl::AssociatedType),
            // Trait-body fn members: `fn f(self) -> T { ... }` (function_definition)
            // or `fn req(self) -> T` (bodyless function_signature).
            "function_definition" | "function_signature" => {
                self.lower_function(child).map(MemberDecl::Fn)
            }
            "port_declaration" => check_and_lower!(
                self,
                child,
                "port",
                self.lower_port(child).map(MemberDecl::Port)
            ),
            "connect_statement" => check_and_lower!(
                self,
                child,
                "connect",
                self.lower_connect(child).map(MemberDecl::Connect)
            ),
            "chain_statement" => check_and_lower!(
                self,
                child,
                "chain",
                self.lower_chain(child).map(MemberDecl::Chain)
            ),
            "constraint_instantiation" => check_and_lower!(
                self,
                child,
                "constraint instantiation",
                self.lower_constraint_inst(child)
                    .map(MemberDecl::ConstraintInst)
            ),
            "meta_block" => check_and_lower!(
                self,
                child,
                "meta",
                self.lower_meta_block(child).map(MemberDecl::MetaBlock)
            ),
            "match_arm_decl_block" => check_and_lower!(
                self,
                child,
                "match arm decl block",
                self.lower_match_arm_decl_group(child)
                    .map(MemberDecl::MatchArmDeclGroup)
            ),
            "forall_statement" => check_and_lower!(
                self,
                child,
                "forall statement",
                self.lower_forall_statement(child)
            ),
            "ERROR" => {
                self.push_error(
                    format!("syntax error: {}", self.node_text(child)),
                    self.span(child),
                );
                None
            }
            _ => None,
        }
    }

    /// Collect members and block-level pragmas from children of a node.
    ///
    /// Returns `(members, pragmas)` — pragma nodes are separated from member nodes
    /// so each block-scoped type can store them independently.
    fn lower_members(&mut self, node: tree_sitter::Node) -> (Vec<MemberDecl>, Vec<Pragma>) {
        let mut members = Vec::new();
        let mut pragmas = Vec::new();
        let mut pending_annotations: Vec<Annotation> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "annotation" => {
                    if let Some(annotation) = self.lower_annotation(child) {
                        pending_annotations.push(annotation);
                    }
                }
                "pragma" => {
                    if let Some(pragma) = self.lower_pragma(child) {
                        pragmas.push(pragma);
                    }
                }
                "ERROR" => {
                    // Consume pending annotations so they don't leak past a syntax error.
                    let _ = std::mem::take(&mut pending_annotations);
                    self.push_error(
                        format!("syntax error: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => {
                    // Drain pending annotations before lowering the member.
                    // If lowering fails (returns None), annotations are still consumed.
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut member) = self.lower_member(child) {
                        match &mut member {
                            MemberDecl::Param(p) => p.annotations = annotations,
                            MemberDecl::Let(l) => l.annotations = annotations,
                            _ => {
                                // Annotations on non-param/non-let members are
                                // silently dropped — no defined semantics yet.
                            }
                        }
                        members.push(member);
                    }
                }
            }
        }
        (members, pragmas)
    }

    fn lower_structure(&mut self, node: tree_sitter::Node) -> Option<StructureDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);

        // Detect 'pub' keyword by checking anonymous children
        let is_pub = self.has_pub_keyword(node);

        // Extract optional type parameters
        let type_params = self.lower_type_parameters(node);

        // Extract optional trait bounds (as TraitBoundRef with type args)
        let trait_bounds = self.find_trait_bound_refs(node);

        let (members, pragmas) = self.lower_members(node);

        let content_hash = self.content_hash(node);

        Some(StructureDef {
            name,
            doc,
            is_pub,
            type_params,
            trait_bounds,
            members,
            span: self.span(node),
            content_hash,
            pragmas,
            annotations: vec![],
        })
    }

    fn lower_occurrence(&mut self, node: tree_sitter::Node) -> Option<OccurrenceDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);
        let is_pub = self.has_pub_keyword(node);
        let type_params = self.lower_type_parameters(node);
        let trait_bounds = self.find_trait_bound_refs(node);
        let (members, pragmas) = self.lower_members(node);
        let content_hash = self.content_hash(node);

        Some(OccurrenceDef {
            name,
            doc,
            is_pub,
            type_params,
            trait_bounds,
            members,
            span: self.span(node),
            content_hash,
            pragmas,
            annotations: vec![],
        })
    }

    // ── Guarded block lowering ─────────────────────────────────

    fn lower_guarded_block(&mut self, node: tree_sitter::Node) -> Option<MemberDecl> {
        let condition_node = node.child_by_field_name("condition")?;
        let condition = self.lower_expr(condition_node)?;

        // Collect members from the main block and else block.
        // The grammar structure is: 'where' condition '{' members... '}' ['else' '{' members... '}']
        // We need to distinguish main block members from else block members.
        let mut main_members = Vec::new();
        let mut else_members = Vec::new();
        let mut in_else = false;
        let mut pending_annotations: Vec<Annotation> = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            // Track when we enter the else block
            if !child.is_named() && self.node_text(child) == "else" {
                in_else = true;
                continue;
            }

            match child.kind() {
                "annotation" => {
                    if let Some(annotation) = self.lower_annotation(child) {
                        pending_annotations.push(annotation);
                    }
                }
                "ERROR" => {
                    let _ = std::mem::take(&mut pending_annotations);
                    self.push_error(
                        format!("syntax error: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    if let Some(mut member) = self.lower_member(child) {
                        match &mut member {
                            MemberDecl::Param(p) => p.annotations = annotations,
                            MemberDecl::Let(l) => l.annotations = annotations,
                            _ => {}
                        }
                        if in_else {
                            else_members.push(member);
                        } else {
                            main_members.push(member);
                        }
                    }
                }
            }
        }

        Some(MemberDecl::GuardedGroup(GuardedGroupDecl {
            condition,
            members: main_members,
            else_members,
            span: self.span(node),
            content_hash: self.content_hash(node),
        }))
    }

    // ── Where clause lowering ─────────────────────────────────

    fn lower_where_clause(&self, node: tree_sitter::Node) -> Option<WhereClause> {
        // Find the where_clause child node within a member declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "where_clause" {
                let condition_node = child.child_by_field_name("condition")?;
                let condition = self.lower_expr(condition_node)?;
                return Some(WhereClause {
                    condition,
                    span: self.span(child),
                });
            }
        }
        None
    }

    // ── Member lowering ─────────────────────────────────────

    /// Shared helper: lower a `_binding_value` CST node (grammar.js:752-755) to an `Expr`.
    ///
    /// This is the **single source of truth** for `auto_keyword` → `ExprKind::Auto` lowering
    /// at the five `_binding_value` grammar slots:
    ///
    /// 1. `param_declaration.default`  — via `lower_param`
    /// 2. `let_declaration.value`      — via `lower_let`
    /// 3. `param_assignment.value`     — via `lower_sub` body loop (value discarded until γ=3806)
    /// 4. `connect_param_assignment.value` — via `lower_connect_body`
    /// 5. `named_argument.value`       — via **two** callers:
    ///    - `lower_named_arg` (named_argument_list path, used by `sub` instantiations)
    ///    - `lower_call_argument` (argument_list path, used by `function_call` / `ad_hoc_selector`)
    ///
    /// PRD §4.2 invariant: lowering must be **identical** across all five sites — same
    /// `ExprKind::Auto { free }` shape, same `free`-flag rule (`modifier` field present?),
    /// same span attribution (`self.span(node)` on the `auto_keyword` node).
    ///
    /// For non-`auto_keyword` nodes the call falls through to `self.lower_expr(node)`,
    /// preserving current behavior at all five sites for ordinary expressions.
    fn lower_binding_value(&self, node: tree_sitter::Node) -> Option<Expr> {
        if node.kind() == "auto_keyword" {
            let free = node.child_by_field_name("modifier").is_some();
            Some(Expr {
                kind: ExprKind::Auto { free },
                span: self.span(node),
            })
        } else {
            self.lower_expr(node)
        }
    }

    fn lower_param(&self, node: tree_sitter::Node) -> Option<ParamDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);

        let type_expr = node
            .child_by_field_name("type")
            .map(|t| self.lower_type_expr_node(t));

        // Silently drops unrecognised defaults via .and_then — intentional divergence
        // from lower_fn_param, which diagnoses them. structure/trait param defaults are
        // compiler-internal (auto_keyword handling) and not user-facing call-site defaults.
        let default = node
            .child_by_field_name("default")
            .and_then(|d| self.lower_binding_value(d));

        let where_clause = self.lower_where_clause(node);

        Some(ParamDecl {
            name,
            doc,
            type_expr,
            default,
            where_clause,
            annotations: Vec::new(),
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_let(&self, node: tree_sitter::Node) -> Option<LetDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);

        // Detect 'pub' keyword by checking anonymous children
        let is_pub = self.has_pub_keyword(node);
        // Detect 'aux' modifier (PRD §2.1, task 3899 step-6).
        let is_aux = self.has_aux_keyword(node);

        let type_expr = node
            .child_by_field_name("type")
            .map(|t| self.lower_type_expr_node(t));

        let value_node = node.child_by_field_name("value")?;
        let value = self.lower_binding_value(value_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(LetDecl {
            name,
            doc,
            is_pub,
            is_aux,
            type_expr,
            value,
            where_clause,
            annotations: Vec::new(),
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_constraint(&self, node: tree_sitter::Node) -> Option<ConstraintDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(ConstraintDecl {
            label: None,
            expr,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_minimize(&self, node: tree_sitter::Node) -> Option<MinimizeDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(MinimizeDecl {
            expr,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_maximize(&self, node: tree_sitter::Node) -> Option<MaximizeDecl> {
        let expr_node = node.child_by_field_name("expr")?;
        let expr = self.lower_expr(expr_node)?;

        let where_clause = self.lower_where_clause(node);

        Some(MaximizeDecl {
            expr,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_sub(&mut self, node: tree_sitter::Node) -> Option<SubDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let struct_node = node.child_by_field_name("structure_name")?;
        let structure_name = self.node_text(struct_node).to_string();

        // Detect collection form: `sub name : List<StructName>`
        // by checking for the "List" keyword token among children.
        let mut is_collection = false;
        {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "List" || self.node_text(child) == "List" {
                    is_collection = true;
                    break;
                }
            }
        }

        // Extract optional type arguments: Box<Bolt> (only for non-collection form)
        let type_args = if is_collection {
            Vec::new()
        } else {
            self.lower_type_args_from_node(node)
        };

        let mut args = Vec::new();
        if !is_collection {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "named_argument_list" {
                    let mut arg_cursor = child.walk();
                    for arg_child in child.children(&mut arg_cursor) {
                        if arg_child.kind() == "named_argument"
                            && let Some(pair) = self.lower_named_arg(arg_child)
                        {
                            args.push(pair);
                        }
                    }
                }
            }
        }

        let where_clause = self.lower_where_clause(node);

        // Lower the optional body field: either `specialization_body` or
        // `keyed_member_block` (task 3929, PRD §2.2).
        //
        // The two body kinds are mutually exclusive by construction:
        //   specialization_body → body: Some(_), keyed_members: empty
        //   keyed_member_block  → body: None,    keyed_members: non-empty
        //   no body field       → body: None,    keyed_members: empty
        let (body, keyed_members) = match node.child_by_field_name("body") {
            None => (None, Vec::new()),
            Some(body_node) if body_node.kind() == "keyed_member_block" => {
                // Keyed block: `{ "k1" => { overrides }  "k2" => { overrides } }`
                // Iterate the named keyed_member_entry children; anonymous `{`/`}`
                // tokens are skipped by `named_children`.
                let mut entries = Vec::new();
                let mut cursor = body_node.walk();
                for entry in body_node.named_children(&mut cursor) {
                    if entry.kind() != "keyed_member_entry" {
                        continue;
                    }
                    let key_node = match entry.child_by_field_name("key") {
                        Some(n) => n,
                        // Missing `key` or `overrides` field can only occur on
                        // ERROR CST nodes (the grammar makes both fields mandatory).
                        // The ERROR node itself surfaces a diagnostic to the user;
                        // silently skipping the entry here keeps downstream consumers
                        // from seeing a half-populated keyed_members Vec.
                        None => continue,
                    };
                    let overrides_node = match entry.child_by_field_name("overrides") {
                        Some(n) => n,
                        None => continue, // same rationale as the `key` arm above
                    };
                    // Unquote the key string_literal.
                    // Reuses the strip-quotes pattern from lower_pragma_value (~lines 1224-1231).
                    //
                    // NOTE: escape sequences (e.g. `"in\"take"`, `"a\nb"`) are NOT
                    // decoded — the raw text between the outer quotes is stored as-is.
                    // This is intentional for v1 (keys are expected to be plain
                    // identifier-like strings with no escapes).  If/when a shared
                    // string-literal unescape helper is introduced, both this site and
                    // lower_pragma_value should route through it; the downstream
                    // E_DUP_MEMBER_KEY / key-comparison work (PRD tasks β/γ) must also
                    // handle escape-decoded vs raw equality.
                    let raw_key = self.node_text(key_node);
                    let key = raw_key
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(raw_key)
                        .to_string();
                    // Lower the override specialization_body via the shared helper.
                    let overrides = self.lower_specialization_body_members(overrides_node);
                    let span = self.span(entry);
                    entries.push(KeyedSubMemberEntry { key, overrides, span });
                }
                (None, entries)
            }
            Some(body_node) => {
                // specialization_body: `{ repeat(param_assignment | _member) }`
                let members = self.lower_specialization_body_members(body_node);
                (Some(members), Vec::new())
            }
        };

        // Detect 'aux' modifier (PRD §2.2, task 3899 step-6).
        let is_aux = self.has_aux_keyword(node);
        // Lower the optional `at <pose>` clause. The grammar exposes the pose
        // expression as a named field "pose" on the sub_declaration node
        // (grammar.js task 3899 step-2). Pattern mirrors other optional-expr
        // members (e.g. lower_port frame_expr, lower_param default).
        let pose_expr = node
            .child_by_field_name("pose")
            .and_then(|n| self.lower_expr(n));

        Some(SubDecl {
            name,
            structure_name,
            type_args,
            args,
            is_collection,
            where_clause,
            body,
            keyed_members,
            is_aux,
            pose_expr,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    /// Lower a `specialization_body` CST node (`{ repeat(param_assignment | _member) }`)
    /// into a `Vec<MemberDecl>`.
    ///
    /// Shared by the `specialization_body` path and the per-entry `overrides` path in
    /// `keyed_member_block` lowering (task 3929) — both block forms parse via the same
    /// `specialization_body` grammar rule and both lower via this helper.
    ///
    /// Dispatch strategy (mirrors the pre-extracted inline logic in `lower_sub`):
    /// - `_member` children → `lower_member` (single source of truth).
    /// - `param_assignment` children → silently skipped for now.
    ///   TODO(task 3573): lower param_assignment once a MemberDecl variant or
    ///   let-rewrite is decided for the `name = value where?` form.
    ///   Exception: `auto_keyword` values invoke `lower_binding_value` for
    ///   centralised auto-keyword tracking (β = task 3804, PRD §4.2), but
    ///   the result is discarded until γ = task 3806 wires the AST plumbing.
    fn lower_specialization_body_members(&mut self, body_node: tree_sitter::Node) -> Vec<MemberDecl> {
        let mut members = Vec::new();
        let mut cursor = body_node.walk();
        for child in body_node.children(&mut cursor) {
            if child.kind() == "param_assignment" {
                if let Some(v) = child.child_by_field_name("value")
                    && v.kind() == "auto_keyword"
                {
                    let _ = self.lower_binding_value(v);
                }
                continue;
            }
            if let Some(member) = self.lower_member(child) {
                members.push(member);
            }
        }
        members
    }

    fn lower_port(&mut self, node: tree_sitter::Node) -> Option<PortDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_node = node.child_by_field_name("type")?;
        let type_name = self.node_text(type_node).to_string();

        // Optional inline direction
        let direction = node
            .child_by_field_name("direction")
            .map(|d| match self.node_text(d) {
                "in" => PortDirection::In,
                "out" => PortDirection::Out,
                "bidi" => PortDirection::Bidi,
                other => {
                    self.push_error(format!("unknown port direction: {}", other), self.span(d));
                    PortDirection::Bidi
                }
            });

        // Optional body: port_body node contains members, direction setting, frame setting
        let (members, body_direction, frame_expr) =
            if let Some(body_node) = node.child_by_field_name("body") {
                self.lower_port_body(body_node)
            } else {
                (Vec::new(), None, None)
            };

        // Body direction overrides inline direction
        let final_direction = body_direction.or(direction);

        Some(PortDecl {
            name,
            direction: final_direction,
            type_name,
            members,
            frame_expr,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_port_body(
        &mut self,
        node: tree_sitter::Node,
    ) -> (Vec<MemberDecl>, Option<PortDirection>, Option<Expr>) {
        let mut members = Vec::new();
        let mut body_direction = None;
        let mut frame_expr = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "param_declaration" => {
                    if let Some(p) = self.lower_param(child) {
                        members.push(MemberDecl::Param(p));
                    }
                }
                "let_declaration" => {
                    if let Some(l) = self.lower_let(child) {
                        members.push(MemberDecl::Let(l));
                    }
                }
                "constraint_declaration" => {
                    if let Some(c) = self.lower_constraint(child) {
                        members.push(MemberDecl::Constraint(c));
                    }
                }
                "port_direction_setting" => {
                    if let Some(value_node) = child.child_by_field_name("value") {
                        body_direction = Some(match self.node_text(value_node) {
                            "in" => PortDirection::In,
                            "out" => PortDirection::Out,
                            "bidi" => PortDirection::Bidi,
                            _ => PortDirection::Bidi,
                        });
                    }
                }
                "port_frame_setting" => {
                    if let Some(value_node) = child.child_by_field_name("value") {
                        frame_expr = self.lower_expr(value_node);
                    }
                }
                "ERROR" => {
                    self.push_error(
                        format!("syntax error in port body: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => self.warn_unexpected_child(child, "port body"),
            }
        }

        (members, body_direction, frame_expr)
    }

    fn lower_connect(&mut self, node: tree_sitter::Node) -> Option<ConnectDecl> {
        let left_node = node.child_by_field_name("left")?;
        let left = self.lower_port_ref(left_node)?;

        let op_node = node.child_by_field_name("operator")?;
        let operator = match self.node_text(op_node) {
            "->" => ConnectOp::Forward,
            "<-" => ConnectOp::Reverse,
            "<->" => ConnectOp::Bidirectional,
            other => {
                self.push_error(
                    format!("unknown connect operator: {}", other),
                    self.span(op_node),
                );
                ConnectOp::Forward
            }
        };

        let right_node = node.child_by_field_name("right")?;
        let right = self.lower_port_ref(right_node)?;

        let connector_type = node
            .child_by_field_name("connector_type")
            .map(|n| self.node_text(n).to_string());

        let (params, port_mappings) = if let Some(body_node) = node.child_by_field_name("body") {
            self.lower_connect_body(body_node)
        } else {
            (Vec::new(), Vec::new())
        };

        Some(ConnectDecl {
            left,
            operator,
            right,
            connector_type,
            params,
            port_mappings,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_port_ref(&self, node: tree_sitter::Node) -> Option<PortRef> {
        // port_ref wraps an _expression, so unwrap to get the actual expression child
        let expr_node = if node.kind() == "port_ref" {
            node.child(0)?
        } else {
            node
        };
        let expr = self.lower_expr(expr_node)?;
        Some(PortRef { expr })
    }

    #[allow(clippy::type_complexity)]
    fn lower_connect_body(
        &mut self,
        node: tree_sitter::Node,
    ) -> (Vec<(String, Expr)>, Vec<(String, String)>) {
        let mut params = Vec::new();
        let mut port_mappings = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "connect_param_assignment" => {
                    if child.has_error() {
                        self.push_error(
                            format!("invalid connect parameter: {}", self.node_text(child)),
                            self.span(child),
                        );
                        continue;
                    }
                    let Some(name_node) = child.child_by_field_name("name") else {
                        self.push_error(
                            format!("connect parameter missing name: {}", self.node_text(child)),
                            self.span(child),
                        );
                        continue;
                    };
                    let name = self.node_text(name_node).to_string();
                    let Some(value_node) = child.child_by_field_name("value") else {
                        self.push_error(
                            format!("connect parameter '{}' missing value", name),
                            self.span(child),
                        );
                        continue;
                    };
                    let Some(value) = self.lower_binding_value(value_node) else {
                        self.push_error(
                            format!("invalid value in connect parameter '{}'", name),
                            self.span(value_node),
                        );
                        continue;
                    };
                    params.push((name, value));
                }
                "port_mapping" => {
                    if child.has_error() {
                        self.push_error(
                            format!("invalid port mapping: {}", self.node_text(child)),
                            self.span(child),
                        );
                        continue;
                    }
                    match (
                        child.child_by_field_name("from"),
                        child.child_by_field_name("to"),
                    ) {
                        (Some(from_node), Some(to_node)) => {
                            let from = self.node_text(from_node).to_string();
                            let to = self.node_text(to_node).to_string();
                            port_mappings.push((from, to));
                        }
                        _ => {
                            self.push_error(
                                format!("incomplete port mapping: {}", self.node_text(child)),
                                self.span(child),
                            );
                        }
                    }
                }
                "ERROR" => {
                    self.push_error(
                        format!("syntax error in connect body: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => self.warn_unexpected_child(child, "connect body"),
            }
        }

        (params, port_mappings)
    }

    fn lower_chain(&mut self, node: tree_sitter::Node) -> Option<ChainDecl> {
        let mut elements = Vec::new();

        // First element
        if let Some(first_node) = node.child_by_field_name("first")
            && let Some(expr) = self.lower_expr(first_node)
        {
            elements.push(expr);
        }

        // Remaining elements: each expression child after '->'
        let mut cursor = node.walk();
        let mut after_arrow = false;
        for child in node.children(&mut cursor) {
            if child.kind() == "->" {
                after_arrow = true;
                continue;
            }
            if after_arrow {
                // Skip if it's the first element (already handled)
                if Some(child.id()) == node.child_by_field_name("first").map(|n| n.id()) {
                    after_arrow = false;
                    continue;
                }
                if let Some(expr) = self.lower_expr(child) {
                    elements.push(expr);
                }
                after_arrow = false;
            }
        }

        if elements.len() < 2 {
            self.push_error(
                "chain requires at least 2 elements".to_string(),
                self.span(node),
            );
            return None;
        }

        Some(ChainDecl {
            elements,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_constraint_inst(&self, node: tree_sitter::Node) -> Option<ConstraintInstDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "named_argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if arg_child.kind() == "named_argument"
                        && let Some(pair) = self.lower_named_arg(arg_child)
                    {
                        args.push(pair);
                    }
                }
            }
        }

        let where_clause = self.lower_where_clause(node);

        Some(ConstraintInstDecl {
            name,
            args,
            where_clause,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    /// Lower a `forall_statement` node.
    ///
    /// Dispatches on the body node's kind:
    /// - `connect_statement` → `MemberDecl::ForallConnect` with `ForallConnectBody::Connect`
    /// - `chain_statement`   → `MemberDecl::ForallConnect` with `ForallConnectBody::Chain`
    /// - `constraint_declaration` → `MemberDecl::ForallConstraint` with `ForallConstraintBody::Constraint`
    /// - `constraint_instantiation` → `MemberDecl::ForallConstraint` with `ForallConstraintBody::Instantiation`
    ///
    /// Disambiguation contract: this lowers `forall ... : connect/chain/constraint/constraint_instantiation`
    /// only; bare `forall x in C: pred` at expression positions remains an `ExprKind::Quantifier`
    /// produced by `lower_quantifier_expression`.
    fn lower_forall_statement(&mut self, node: tree_sitter::Node) -> Option<MemberDecl> {
        let variable_node = node.child_by_field_name("variable")?;
        let variable = self.node_text(variable_node).to_string();

        let collection_node = node.child_by_field_name("collection")?;
        let collection = self.lower_expr(collection_node)?;

        let body_node = node.child_by_field_name("body")?;

        match body_node.kind() {
            "connect_statement" => {
                let connect =
                    check_and_lower!(self, body_node, "connect", self.lower_connect(body_node))?;
                Some(MemberDecl::ForallConnect(ForallConnectDecl {
                    variable,
                    collection,
                    body: ForallConnectBody::Connect(Box::new(connect)),
                    span: self.span(node),
                    content_hash: self.content_hash(node),
                }))
            }
            "chain_statement" => {
                let chain =
                    check_and_lower!(self, body_node, "chain", self.lower_chain(body_node))?;
                Some(MemberDecl::ForallConnect(ForallConnectDecl {
                    variable,
                    collection,
                    body: ForallConnectBody::Chain(chain),
                    span: self.span(node),
                    content_hash: self.content_hash(node),
                }))
            }
            "constraint_declaration" => {
                let constraint = check_and_lower!(
                    self,
                    body_node,
                    "constraint",
                    self.lower_constraint(body_node)
                )?;
                Some(MemberDecl::ForallConstraint(ForallConstraintDecl {
                    variable,
                    collection,
                    body: ForallConstraintBody::Constraint(constraint),
                    span: self.span(node),
                    content_hash: self.content_hash(node),
                }))
            }
            "constraint_instantiation" => {
                let inst = check_and_lower!(
                    self,
                    body_node,
                    "constraint instantiation",
                    self.lower_constraint_inst(body_node)
                )?;
                Some(MemberDecl::ForallConstraint(ForallConstraintDecl {
                    variable,
                    collection,
                    body: ForallConstraintBody::Instantiation(inst),
                    span: self.span(node),
                    content_hash: self.content_hash(node),
                }))
            }
            other => {
                self.push_error(
                    format!("unsupported forall body kind: {}", other),
                    self.span(body_node),
                );
                None
            }
        }
    }

    fn lower_meta_block(&self, node: tree_sitter::Node) -> Option<MetaBlockDecl> {
        let mut entries = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "meta_entry" {
                let key_node = child.child_by_field_name("key");
                let value_node = child.child_by_field_name("value");
                if let (Some(k), Some(v)) = (key_node, value_node) {
                    let key = self.node_text(k).to_string();
                    let raw = self.node_text(v);
                    // Strip outer quotes safely
                    let value = raw
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(raw)
                        .to_string();
                    entries.push((key, value));
                }
            }
        }
        Some(MetaBlockDecl {
            entries,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_named_arg(&self, node: tree_sitter::Node) -> Option<(String, Expr)> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();
        let value_node = node.child_by_field_name("value")?;
        let value = self.lower_binding_value(value_node)?;
        Some((name, value))
    }

    // ── Expression lowering ─────────────────────────────────

    fn lower_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        match node.kind() {
            "binary_expression" => self.lower_binary_expr(node),
            "unary_expression" => self.lower_unary_expr(node),
            "range_expression" => self.lower_range_expr(node),
            "conditional_expression" => self.lower_conditional(node),
            "match_expression" => self.lower_match_expr(node),
            "lambda_expression" => self.lower_lambda_expression(node),
            "quantifier_expression" => self.lower_quantifier_expression(node),
            "quantity_literal" => self.lower_quantity_literal(node),
            "number_literal" => self.lower_number_literal(node),
            "string_literal" => self.lower_string_literal(node),
            "bool_literal" => self.lower_bool_literal(node),
            "identifier" => self.lower_identifier(node),
            "function_call" => self.lower_function_call(node),
            "list_literal" => self.lower_list_literal(node),
            "set_literal" => self.lower_set_literal(node),
            "map_literal" => self.lower_map_literal(node),
            "ad_hoc_selector" => self.lower_ad_hoc_selector(node),
            "index_access" => self.lower_index_access(node),
            "member_access" => self.lower_member_access(node),
            "qualified_access" => self.lower_qualified_access(node),
            "instance_qualified_access" => self.lower_instance_qualified_access(node),
            "trait_method_call" => self.lower_trait_method_call(node),
            "parenthesized_expression" => {
                // Unwrap parenthesized expression — find the inner expression
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() && child.kind() != "(" && child.kind() != ")" {
                        return self.lower_expr(child);
                    }
                }
                None
            }
            // Unknown node kind — skip
            _ => None,
        }
    }

    fn lower_binary_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        let left_node = node.child_by_field_name("left")?;
        let op_node = node.child_by_field_name("op")?;
        let right_node = node.child_by_field_name("right")?;

        let left = self.lower_expr(left_node)?;
        let right = self.lower_expr(right_node)?;
        let op = self.node_text(op_node).to_string();

        Some(Expr {
            kind: ExprKind::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span: self.span(node),
        })
    }

    fn lower_unary_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        let op_node = node.child_by_field_name("op")?;
        let operand_node = node.child_by_field_name("operand")?;

        let op = self.node_text(op_node).to_string();
        let operand = self.lower_expr(operand_node)?;

        Some(Expr {
            kind: ExprKind::UnOp {
                op,
                operand: Box::new(operand),
            },
            span: self.span(node),
        })
    }

    fn lower_range_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        // Discriminate two-sided vs single-sided by named-field presence:
        // two-sided ranges (a..b, a..<b) carry `lower`/`upper` fields;
        // single-sided prefix ranges (>x, >=x, <x, <=x) carry `op`/`bound` fields.
        // (mirrors grammar.js:929 — absence of lower/upper fields is the discriminator)
        if let (Some(lower_node), Some(upper_node)) = (
            node.child_by_field_name("lower"),
            node.child_by_field_name("upper"),
        ) {
            // Two-sided form: existing logic, kept intact.
            let lower = self.lower_expr(lower_node)?;
            let upper = self.lower_expr(upper_node)?;
            // Determine inclusive/exclusive by checking for "..<" token
            let mut exclusive_upper = false;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if !child.is_named() && self.node_text(child) == "..<" {
                    exclusive_upper = true;
                    break;
                }
            }
            Some(Expr {
                kind: ExprKind::Range {
                    lower: Some(Box::new(lower)),
                    upper: Some(Box::new(upper)),
                    lower_inclusive: true,
                    upper_inclusive: !exclusive_upper,
                },
                span: self.span(node),
            })
        } else {
            // Single-sided prefix form: `op` names the operator, `bound` is the operand.
            // D5 inclusivity mapping: absent-side *_inclusive = true (vacuous).
            let op_node = node.child_by_field_name("op")?;
            let bound_node = node.child_by_field_name("bound")?;
            let bound = self.lower_expr(bound_node)?;
            let op = self.node_text(op_node);
            let (lower, upper, lower_inclusive, upper_inclusive) = match op {
                ">" => (Some(Box::new(bound)), None, false, true),
                ">=" => (Some(Box::new(bound)), None, true, true),
                "<" => (None, Some(Box::new(bound)), true, false),
                "<=" => (None, Some(Box::new(bound)), true, true),
                _ => return None,
            };
            Some(Expr {
                kind: ExprKind::Range {
                    lower,
                    upper,
                    lower_inclusive,
                    upper_inclusive,
                },
                span: self.span(node),
            })
        }
    }

    fn lower_conditional(&self, node: tree_sitter::Node) -> Option<Expr> {
        let condition_node = node.child_by_field_name("condition")?;
        let then_node = node.child_by_field_name("then")?;
        let else_node = node.child_by_field_name("else")?;

        let condition = self.lower_expr(condition_node)?;
        let then_branch = self.lower_expr(then_node)?;
        let else_branch = self.lower_expr(else_node)?;

        Some(Expr {
            kind: ExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span: self.span(node),
        })
    }

    fn lower_lambda_expression(&self, node: tree_sitter::Node) -> Option<Expr> {
        // Collect lambda_param children
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "lambda_param"
                && let Some(param) = self.lower_lambda_param(child)
            {
                params.push(param);
            }
        }

        let body_node = node.child_by_field_name("body")?;
        let body = self.lower_expr(body_node)?;

        Some(Expr {
            kind: ExprKind::Lambda {
                params,
                body: Box::new(body),
            },
            span: self.span(node),
        })
    }

    fn lower_lambda_param(&self, node: tree_sitter::Node) -> Option<LambdaParam> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_expr = node.child_by_field_name("type").map(|t| {
            let ident = if t.kind() == "type_expr" {
                t.child(0).unwrap_or(t)
            } else {
                t
            };
            TypeExpr {
                kind: TypeExprKind::Named {
                    name: self.node_text(ident).to_string(),
                    type_args: vec![],
                },
                span: self.span(ident),
            }
        });

        Some(LambdaParam {
            name,
            type_expr,
            span: self.span(node),
        })
    }

    fn lower_quantifier_expression(&self, node: tree_sitter::Node) -> Option<Expr> {
        let quantifier_node = node.child_by_field_name("quantifier")?;
        let kind = match self.node_text(quantifier_node) {
            "forall" => QuantifierKind::ForAll,
            "exists" => QuantifierKind::Exists,
            _ => return None,
        };

        let variable_node = node.child_by_field_name("variable")?;
        let variable = self.node_text(variable_node).to_string();

        let collection_node = node.child_by_field_name("collection")?;
        let collection = self.lower_expr(collection_node)?;

        let predicate_node = node.child_by_field_name("predicate")?;
        let predicate = self.lower_expr(predicate_node)?;

        Some(Expr {
            kind: ExprKind::Quantifier {
                kind,
                variable,
                collection: Box::new(collection),
                predicate: Box::new(predicate),
            },
            span: self.span(node),
        })
    }

    fn lower_match_expr(&self, node: tree_sitter::Node) -> Option<Expr> {
        let discriminant_node = node.child_by_field_name("discriminant")?;
        let discriminant = self.lower_expr(discriminant_node)?;

        let mut arms = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "match_arm"
                && let Some(arm) = self.lower_match_arm(child)
            {
                arms.push(arm);
            }
        }

        Some(Expr {
            kind: ExprKind::Match {
                discriminant: Box::new(discriminant),
                arms,
            },
            span: self.span(node),
        })
    }

    fn lower_match_arm(&self, node: tree_sitter::Node) -> Option<MatchArm> {
        let pattern_node = node.child_by_field_name("pattern")?;
        let body_node = node.child_by_field_name("body")?;

        let body = self.lower_expr(body_node)?;

        // Collect structured MatchPattern values from the match_pattern node.
        // Choices:
        //   '_'                              → [Wildcard]
        //   variant_binding_pattern child    → [VariantBind { name, binders }]
        //   identifier(s) separated by '|'  → [Variant(n), ...] one per identifier
        let mut patterns: Vec<MatchPattern> = Vec::new();
        let pattern_text = self.node_text(pattern_node).trim();

        if pattern_text == "_" {
            patterns.push(MatchPattern::Wildcard);
        } else {
            let mut cursor = pattern_node.walk();
            for child in pattern_node.children(&mut cursor) {
                match child.kind() {
                    "variant_binding_pattern" => {
                        // Named-field payload binding: `Circle { radius: r }`.
                        let variant_node =
                            child.child_by_field_name("variant")?;
                        let name = self.node_text(variant_node).to_string();

                        // Collect (field, binder) pairs from field_binding children.
                        let mut binders: Vec<(String, String)> = Vec::new();
                        let mut fb_cursor = child.walk();
                        for fb_child in child.children(&mut fb_cursor) {
                            if fb_child.kind() == "field_binding" {
                                let field_node =
                                    fb_child.child_by_field_name("field")?;
                                let binder_node =
                                    fb_child.child_by_field_name("binder")?;
                                binders.push((
                                    self.node_text(field_node).to_string(),
                                    self.node_text(binder_node).to_string(),
                                ));
                            }
                        }
                        patterns.push(MatchPattern::VariantBind { name, binders });
                    }
                    "identifier" => {
                        patterns.push(MatchPattern::Variant(
                            self.node_text(child).to_string(),
                        ));
                    }
                    _ => {}
                }
            }
        }

        if patterns.is_empty() {
            return None;
        }

        Some(MatchArm {
            patterns,
            body,
            span: self.span(node),
        })
    }

    fn lower_match_arm_decl_group(
        &self,
        node: tree_sitter::Node,
    ) -> Option<MatchArmDeclGroupDecl> {
        let discriminant_node = node.child_by_field_name("discriminant")?;
        let discriminant = self.lower_expr(discriminant_node).or_else(|| {
            // A well-formed discriminant node that lower_expr cannot produce an
            // Expr for indicates a grammar/lowering mismatch.  Surface it rather
            // than silently yielding a phantom non-exhaustive-match later.
            self.push_error(
                format!(
                    "unable to lower match discriminant: {}",
                    self.node_text(discriminant_node)
                ),
                self.span(discriminant_node),
            );
            None
        })?;

        let mut arms = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "match_arm_decl_arm" {
                match self.lower_match_arm_decl_arm(child) {
                    Some(arm) => arms.push(arm),
                    None if !child.has_error() => {
                        // Arm has no CST error but lowering failed — grammar/lowering
                        // mismatch.  Push a diagnostic so the mismatch surfaces rather
                        // than producing a silent non-exhaustive match.
                        self.push_error(
                            format!(
                                "unable to lower match arm: {}",
                                self.node_text(child)
                            ),
                            self.span(child),
                        );
                    }
                    None => {} // child.has_error() — already caught by check_and_lower! at dispatch
                }
            }
        }

        Some(MatchArmDeclGroupDecl {
            discriminant,
            arms,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    fn lower_match_arm_decl_arm(
        &self,
        node: tree_sitter::Node,
    ) -> Option<MatchArmDeclArmDecl> {
        let pattern_node = node.child_by_field_name("pattern")?;
        let member_node = node.child_by_field_name("member")?;

        // Collect patterns from the match_pattern node.
        // Pattern is either '_' (wildcard) or one or more identifiers separated by '|'.
        let mut patterns = Vec::new();
        let pattern_text = self.node_text(pattern_node).trim();

        if pattern_text == "_" {
            patterns.push("_".to_string());
        } else {
            // Iterate children (identifiers) of the match_pattern node.
            let mut cursor = pattern_node.walk();
            for child in pattern_node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    patterns.push(self.node_text(child).to_string());
                }
            }
        }

        if patterns.is_empty() {
            return None;
        }

        // Build a SubDecl from the match_arm_sub_decl node.
        // The grammar restricts match_arm_sub_decl to: 'sub', name, ':', structure_name.
        // No type_args, args, where_clause, or body are permitted.
        let name_node = member_node.child_by_field_name("name")?;
        let structure_name_node = member_node.child_by_field_name("structure_name")?;

        let sub_decl = SubDecl {
            name: self.node_text(name_node).to_string(),
            structure_name: self.node_text(structure_name_node).to_string(),
            type_args: vec![],
            args: vec![],
            is_collection: false,
            where_clause: None,
            body: None,
            keyed_members: Vec::new(),
            is_aux: false,
            pose_expr: None,
            span: self.span(member_node),
            content_hash: self.content_hash(member_node),
        };

        Some(MatchArmDeclArmDecl {
            patterns,
            member: Box::new(MemberDecl::Sub(sub_decl)),
            span: self.span(node),
        })
    }

    /// Strip `_` digit-separator characters from a numeric literal token and
    /// parse the result as `f64`.
    ///
    /// The grammar (`tree-sitter-reify/grammar.js`) accepts `_` between digit
    /// groups (e.g. `1_000_000`, `0.000_001`, `1_000e1_0`), but `f64::from_str`
    /// rejects `_` in raw form.  This helper strips them before parsing so all
    /// three lowering sites — `lower_number_literal`, `lower_quantity_literal`,
    /// and `lower_pragma_value` — share the same path and cannot diverge.
    ///
    /// The `is_real` classification (`.`/`e`/`E` scan) in `lower_number_literal`
    /// is unaffected: `_` is never `.`, `e`, or `E`, so the scan result is
    /// identical whether run on the original or stripped text.
    fn strip_underscores_and_parse(text: &str) -> Option<f64> {
        if text.contains('_') {
            text.replace('_', "").parse().ok()
        } else {
            text.parse().ok()
        }
    }

    /// Parse a `number_literal` token text into `(value, is_real)`.
    ///
    /// Dispatches on the radix prefix before attempting `f64` conversion:
    ///
    /// - **Hex** (`0x`/`0X`): strips the prefix and any `_` separators, parses
    ///   via `u64::from_str_radix(.., 16)`, returns `(n as f64, false)`.
    /// - **Binary** (`0b`/`0B`): same, with radix 2.
    /// - **Decimal** (everything else): delegates to
    ///   [`Self::strip_underscores_and_parse`] for `f64::from_str` (preserving
    ///   β/3912 `_`-separator support), then classifies `is_real` by scanning
    ///   the *original* text for `.`, `e`, or `E`.
    ///
    /// # D4 is_real guard
    ///
    /// `is_real` is forced `false` on both radix branches regardless of the
    /// token text.  Without this guard, `0xBEEF` / `0xe` would false-positive
    /// as `Real` due to the `E`/`e` in their hex digits.  Hex/binary literals
    /// are integer-only by grammar (no fractional/exponent form), so
    /// `is_real = false` is always correct on the radix branches.
    ///
    /// # Precision
    ///
    /// Values up to `u64::MAX` are parsed via `u64::from_str_radix`; values
    /// exceeding `u64::MAX` are accumulated as `f64` directly (matching the
    /// decimal path's `f64::parse` approach) so they flow through
    /// `classify_number_literal`'s `LossyReal` path rather than returning
    /// `None` and silently dropping the expression.
    ///
    /// Values beyond 2^53 are stored as `(n as f64)` — a lossy conversion.
    ///
    /// **i64 round-trip boundary:** `classify_number_literal`
    /// (`reify-ast/src/decl.rs`) tests `value == (value as i64) as f64`.
    /// Rust's `as i64` saturates at `i64::MAX`, and `(i64::MAX) as f64`
    /// rounds back to 2^63, so values ≥ 2^63 pass the round-trip check
    /// falsely and are classified as `Int(i64::MAX)` instead of `LossyReal`.
    /// This is a pre-existing limitation in `reify-ast` outside this task's
    /// scope; the `0x8000000000000000` lowering test only validates that this
    /// function itself does not return `None` for that value.
    fn parse_number_literal_text(text: &str) -> Option<(f64, bool)> {
        let parse_radix = |digits: &str, radix: u32| -> Option<f64> {
            let stripped: String = digits.chars().filter(|c| *c != '_').collect();
            if let Ok(n) = u64::from_str_radix(&stripped, radix) {
                Some(n as f64)
            } else {
                // Value exceeds u64::MAX — accumulate as f64 so over-range
                // radix literals flow to classify_number_literal's LossyReal
                // path rather than silently returning None (matches the decimal
                // path, which accepts arbitrary magnitude via f64::parse →
                // finite or f64::INFINITY).
                let radix_f = radix as f64;
                let mut acc = 0.0_f64;
                for ch in stripped.chars() {
                    let digit = ch.to_digit(radix)? as f64;
                    acc = acc * radix_f + digit;
                }
                Some(acc)
            }
        };

        if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            return Some((parse_radix(digits, 16)?, false));
        }
        if let Some(digits) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
            return Some((parse_radix(digits, 2)?, false));
        }

        // Decimal branch: preserve `_`-separator support via the shared helper.
        let value = Self::strip_underscores_and_parse(text)?;
        let is_real = text.contains('.') || text.contains('e') || text.contains('E');
        Some((value, is_real))
    }

    fn lower_quantity_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let value_node = node.child_by_field_name("value")?;
        let unit_node = node.child_by_field_name("unit")?;

        // Use the shared radix-aware helper so that hex/binary quantity values
        // (e.g. `0xFFmm`, `0b1010mm`) lower correctly (PRD D3/D4, task 3913/δ).
        // strip_underscores_and_parse returns None for "0xFF", so using it here
        // would silently drop radix quantity literals — the exact gap the γ
        // grammar (task 3910) opened when it made `0xFFmm` parse as
        // quantity_literal(number_literal "0xFF", unit_expr "mm").
        // The `_is_real` component is discarded: QuantityLiteral has no is_real field.
        let (value, _is_real) = Self::parse_number_literal_text(self.node_text(value_node))?;
        let unit = self.lower_unit_expr(unit_node)?;

        Some(Expr {
            kind: ExprKind::QuantityLiteral { value, unit },
            span: self.span(node),
        })
    }

    /// Lower a `unit_expr` CST node into a structured [`UnitExpr`] tree.
    ///
    /// Probe order mirrors the grammar's precedence (PRD
    /// `docs/prds/unit-expressions.md` §3.2/§4.1; task α corpus
    /// `tree-sitter-reify/test/corpus/unit_expr.txt`):
    ///   1. **Pow** — `base ^ exponent`. Probed first because the pow arm also
    ///      carries an `op` field (the `^`), but is uniquely identified by the
    ///      presence of `base` + `exponent` fields.
    ///   2. **Mul/Div** — `left (*|/) right`, left-associative. Dispatch on the
    ///      operator's source TEXT, not node kind: the `op` field aliases the two
    ///      external-scanner tokens (`_unit_mul_op` / `_unit_div_op`).
    ///   3. **Paren / bare unit** — a parenthesised `unit_expr` is unwrapped
    ///      transparently (no `Paren` variant — parens carry no semantics); a
    ///      `unit_name` child becomes [`UnitExpr::Unit`].
    ///
    /// Returns `None` on a malformed CST so `?` propagates a parse failure
    /// cleanly, matching the other `lower_*` helpers.
    fn lower_unit_expr(&self, node: tree_sitter::Node) -> Option<UnitExpr> {
        // 1. Pow: `base ^ exponent`.
        if let (Some(base_node), Some(exp_node)) = (
            node.child_by_field_name("base"),
            node.child_by_field_name("exponent"),
        ) {
            let base = self.lower_unit_expr(base_node)?;
            // grammar's `signed_integer` is `-?\d+`, so this parse is total in practice.
            let exponent: i32 = self.node_text(exp_node).parse().ok()?;
            return Some(UnitExpr::Pow(Box::new(base), exponent));
        }

        // 2. Mul/Div: `left (*|/) right`, left-associative. The `op` field aliases
        //    the external-scanner tokens (`_unit_mul_op` / `_unit_div_op`), which
        //    `child_by_field_name` does NOT expose — so detect the arm by the
        //    `left`+`right` fields and read the operator from the source slice
        //    between the two operands. Units are contiguous (no whitespace inside
        //    a unit_expr), so that slice is exactly `*` or `/`.
        if let (Some(left_node), Some(right_node)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) {
            let left = self.lower_unit_expr(left_node)?;
            let right = self.lower_unit_expr(right_node)?;
            let op_text = self
                .source
                .get(left_node.end_byte()..right_node.start_byte())?;
            return if op_text.contains('/') {
                Some(UnitExpr::Div(Box::new(left), Box::new(right)))
            } else if op_text.contains('*') {
                Some(UnitExpr::Mul(Box::new(left), Box::new(right)))
            } else {
                None
            };
        }

        // 3. Paren or bare unit: walk named children. A `unit_name` child is a
        //    bare unit; an inner `unit_expr` child is a parenthesised group that
        //    we unwrap by recursing (parens are anonymous tokens, not children).
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "unit_name" => {
                    return Some(UnitExpr::Unit(self.node_text(child).to_string()));
                }
                "unit_expr" => return self.lower_unit_expr(child),
                _ => {}
            }
        }
        None
    }

    fn lower_number_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let text = self.node_text(node);
        // Dispatch through the radix-aware helper (task 3913 / δ).
        //
        // `parse_number_literal_text` handles:
        //   - Hex (0x/0X): u64::from_str_radix(.., 16), is_real = false
        //   - Binary (0b/0B): u64::from_str_radix(.., 2), is_real = false
        //   - Decimal: strip_underscores_and_parse + `.`/`e`/`E` scan
        //
        // is_real is forced false on radix branches (D4 guard) so that hex
        // tokens containing `e`/`E` (e.g. 0xBEEF, 0xe) do not false-positive
        // as Real literals.  The decimal branch preserves β/3912 `_`-separator
        // support and the `.`/`e`/`E` is_real scan on the original text.
        let (value, is_real) = Self::parse_number_literal_text(text)?;
        Some(Expr {
            kind: ExprKind::NumberLiteral { value, is_real },
            span: self.span(node),
        })
    }

    fn lower_string_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let text = self.node_text(node);
        // Strip outer quotes safely (error recovery can produce malformed nodes)
        let s = text
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(text)
            .to_string();
        Some(Expr {
            kind: ExprKind::StringLiteral(s),
            span: self.span(node),
        })
    }

    fn lower_bool_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let value = match self.node_text(node) {
            "true" => true,
            "false" => false,
            _ => return None,
        };
        Some(Expr {
            kind: ExprKind::BoolLiteral(value),
            span: self.span(node),
        })
    }

    fn lower_identifier(&self, node: tree_sitter::Node) -> Option<Expr> {
        let name = self.node_text(node).to_string();
        Some(Expr {
            kind: ExprKind::Ident(name),
            span: self.span(node),
        })
    }

    fn lower_function_call(&self, node: tree_sitter::Node) -> Option<Expr> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if let Some(expr) = self.lower_call_argument(arg_child) {
                        args.push(expr);
                    }
                }
            }
        }

        Some(Expr {
            kind: ExprKind::FunctionCall { name, args },
            span: self.span(node),
        })
    }

    /// Lower a single child of `argument_list`, which may be either a bare
    /// `_expression` or a `named_argument`. For named arguments, the name is
    /// stripped and only the value is kept as a positional arg — matching the
    /// positional-only shape of `ExprKind::FunctionCall`.
    ///
    /// The `named_argument` branch delegates to `lower_binding_value` (not
    /// `lower_expr`), making this the **second AST-observable caller** of grammar
    /// slot 5 (`named_argument.value`). The first caller is `lower_named_arg`
    /// (via `named_argument_list` for `sub` instantiations). See
    /// `lower_binding_value`'s doc-comment for the full two-caller enumeration.
    fn lower_call_argument(&self, node: tree_sitter::Node) -> Option<Expr> {
        if !node.is_named() {
            return None;
        }
        if node.kind() == "named_argument" {
            let value_node = node.child_by_field_name("value")?;
            return self.lower_binding_value(value_node);
        }
        self.lower_expr(node)
    }

    fn lower_list_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let mut elements = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named()
                && let Some(expr) = self.lower_expr(child)
            {
                elements.push(expr);
            }
        }
        Some(Expr {
            kind: ExprKind::ListLiteral(elements),
            span: self.span(node),
        })
    }

    fn lower_set_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let mut elements = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named()
                && let Some(expr) = self.lower_expr(child)
            {
                elements.push(expr);
            }
        }
        Some(Expr {
            kind: ExprKind::SetLiteral(elements),
            span: self.span(node),
        })
    }

    fn lower_map_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let mut entries = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "map_entry"
                && let Some(entry) = self.lower_map_entry(child)
            {
                entries.push(entry);
            }
        }
        Some(Expr {
            kind: ExprKind::MapLiteral(entries),
            span: self.span(node),
        })
    }

    fn lower_map_entry(&self, node: tree_sitter::Node) -> Option<(Expr, Expr)> {
        let key_node = node.child_by_field_name("key")?;
        let value_node = node.child_by_field_name("value")?;
        let key = self.lower_expr(key_node)?;
        let value = self.lower_expr(value_node)?;
        Some((key, value))
    }

    fn lower_ad_hoc_selector(&self, node: tree_sitter::Node) -> Option<Expr> {
        let base_node = node.child_by_field_name("base")?;
        let selector_node = node.child_by_field_name("selector")?;
        let base = self.lower_expr(base_node)?;
        let selector = self.node_text(selector_node).to_string();

        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if let Some(expr) = self.lower_call_argument(arg_child) {
                        args.push(expr);
                    }
                }
            }
        }

        Some(Expr {
            kind: ExprKind::AdHocSelector {
                base: Box::new(base),
                selector,
                args,
            },
            span: self.span(node),
        })
    }

    fn lower_index_access(&self, node: tree_sitter::Node) -> Option<Expr> {
        let object_node = node.child_by_field_name("object")?;
        let index_node = node.child_by_field_name("index")?;
        let object = self.lower_expr(object_node)?;
        let index = self.lower_expr(index_node)?;
        Some(Expr {
            kind: ExprKind::IndexAccess {
                object: Box::new(object),
                index: Box::new(index),
            },
            span: self.span(node),
        })
    }

    fn lower_qualified_access(&self, node: tree_sitter::Node) -> Option<Expr> {
        let qualifier_node = node.child_by_field_name("qualifier")?;
        let member_node = node.child_by_field_name("member")?;

        let qualifier = self.lower_expr(qualifier_node)?;
        let member = self.node_text(member_node).to_string();

        Some(Expr {
            kind: ExprKind::QualifiedAccess {
                qualifier: Box::new(qualifier),
                member,
            },
            span: self.span(node),
        })
    }

    fn lower_instance_qualified_access(&self, node: tree_sitter::Node) -> Option<Expr> {
        let object_node = node.child_by_field_name("object")?;
        let qualified_node = node.child_by_field_name("qualified")?;

        // Validate CST node kind — tree-sitter error recovery can violate grammar invariants.
        // Emit a specific diagnostic so the user knows what went wrong.
        if qualified_node.kind() != "qualified_access" {
            self.push_error(
                "instance qualified access requires a qualified_access (::) inside the parentheses"
                    .to_string(),
                self.span(node),
            );
            return None;
        }

        let object = self.lower_expr(object_node)?;
        let qualified = self.lower_expr(qualified_node)?;

        // If the CST kind check passed, lowering MUST produce QualifiedAccess.
        // A mismatch here indicates a bug in the lowering code, not invalid user input.
        debug_assert!(
            matches!(&qualified.kind, ExprKind::QualifiedAccess { .. }),
            "CST kind was 'qualified_access' but lowered to {:?}",
            qualified.kind
        );

        Some(Expr {
            kind: ExprKind::InstanceQualifiedAccess {
                object: Box::new(object),
                qualified: Box::new(qualified),
            },
            span: self.span(node),
        })
    }

    /// Lower a `trait_method_call` CST node to either `TraitStaticCall` or
    /// `TraitMethodCall`, depending on whether the `callee` field is a
    /// `qualified_access` (static) or `instance_qualified_access` (instance).
    ///
    /// Grammar: `trait_method_call` has:
    /// - field `callee`: `choice(qualified_access, instance_qualified_access)`
    /// - child `argument_list` (shared with `function_call`)
    fn lower_trait_method_call(&self, node: tree_sitter::Node) -> Option<Expr> {
        let callee_node = node.child_by_field_name("callee")?;

        // Collect positional args from the `argument_list` child (same logic as
        // `lower_function_call`, reusing the existing `lower_call_argument` helper).
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    if let Some(expr) = self.lower_call_argument(arg_child) {
                        args.push(expr);
                    }
                }
            }
        }

        match callee_node.kind() {
            "qualified_access" => {
                // Static form: `Trait::method(args)` — callee is bare qualified_access.
                let qualifier_node = callee_node.child_by_field_name("qualifier")?;
                let member_node = callee_node.child_by_field_name("member")?;
                let trait_name = self.node_text(qualifier_node).to_string();
                let method = self.node_text(member_node).to_string();
                Some(Expr {
                    kind: ExprKind::TraitStaticCall {
                        trait_name,
                        method,
                        args,
                    },
                    span: self.span(node),
                })
            }
            "instance_qualified_access" => {
                // Instance form: `obj.(Trait::method)(args)`.
                let object_node = callee_node.child_by_field_name("object")?;
                let qualified_node = callee_node.child_by_field_name("qualified")?;

                // The inner `qualified` must be a `qualified_access` — validated by grammar,
                // but guarded defensively.
                if qualified_node.kind() != "qualified_access" {
                    self.push_error(
                        "trait method call: expected 'Trait::method' form inside parentheses"
                            .to_string(),
                        self.span(callee_node),
                    );
                    return None;
                }
                let inner_qualifier = qualified_node.child_by_field_name("qualifier")?;
                let inner_member = qualified_node.child_by_field_name("member")?;
                let trait_name = self.node_text(inner_qualifier).to_string();
                let method = self.node_text(inner_member).to_string();

                let object = self.lower_expr(object_node)?;
                Some(Expr {
                    kind: ExprKind::TraitMethodCall {
                        object: Box::new(object),
                        trait_name,
                        method,
                        args,
                    },
                    span: self.span(node),
                })
            }
            other => {
                self.push_error(
                    format!(
                        "trait_method_call: unexpected callee kind '{}'; \
                         expected qualified_access or instance_qualified_access",
                        other
                    ),
                    self.span(callee_node),
                );
                None
            }
        }
    }

    fn lower_member_access(&self, node: tree_sitter::Node) -> Option<Expr> {
        let object_node = node.child_by_field_name("object")?;
        let member_node = node.child_by_field_name("member")?;

        // Check if the object is an identifier that matches a known enum name.
        // If so, produce EnumAccess instead of MemberAccess.
        if object_node.kind() == "identifier" {
            let object_text = self.node_text(object_node);
            if self.known_enums.contains(object_text) {
                let variant = self.node_text(member_node).to_string();
                return Some(Expr {
                    kind: ExprKind::EnumAccess {
                        type_name: object_text.to_string(),
                        variant,
                    },
                    span: self.span(node),
                });
            }
        }

        let object = self.lower_expr(object_node)?;
        let member = self.node_text(member_node).to_string();

        Some(Expr {
            kind: ExprKind::MemberAccess {
                object: Box::new(object),
                member,
            },
            span: self.span(node),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: count ERROR nodes in a tree-sitter tree.
    fn count_errors(node: tree_sitter::Node) -> usize {
        let mut count = if node.is_error() || node.is_missing() {
            1
        } else {
            0
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            count += count_errors(child);
        }
        count
    }

    fn parse_bracket() -> ParsedModule {
        let source = reify_test_support::bracket_source();
        parse(source, ModulePath::single("bracket"))
    }

    #[test]
    fn ts_parse_produces_correct_structure() {
        let module = parse_bracket();
        assert!(
            module.errors.is_empty(),
            "expected no errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 1);

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        assert_eq!(structure.name, "Bracket");
        assert_eq!(structure.members.len(), 10);

        let params: Vec<_> = structure
            .members
            .iter()
            .filter(|m| matches!(m, MemberDecl::Param(_)))
            .collect();
        let lets: Vec<_> = structure
            .members
            .iter()
            .filter(|m| matches!(m, MemberDecl::Let(_)))
            .collect();
        let constraints: Vec<_> = structure
            .members
            .iter()
            .filter(|m| matches!(m, MemberDecl::Constraint(_)))
            .collect();

        assert_eq!(params.len(), 5, "expected 5 params");
        assert_eq!(lets.len(), 2, "expected 2 lets");
        assert_eq!(constraints.len(), 3, "expected 3 constraints");

        // Verify member names in order
        let names: Vec<String> = structure
            .members
            .iter()
            .map(|m| match m {
                MemberDecl::Param(p) => format!("param:{}", p.name),
                MemberDecl::Let(l) => format!("let:{}", l.name),
                MemberDecl::Constraint(_) => "constraint".into(),
                MemberDecl::ConstraintInst(ci) => format!("constraint_inst:{}", ci.name),
                MemberDecl::Sub(s) => format!("sub:{}", s.name),
                MemberDecl::Minimize(_) => "minimize".into(),
                MemberDecl::Maximize(_) => "maximize".into(),
                MemberDecl::GuardedGroup(_) => "guarded_group".into(),
                MemberDecl::AssociatedType(a) => format!("type:{}", a.name),
                MemberDecl::Port(p) => format!("port:{}", p.name),
                MemberDecl::Connect(_) => "connect".into(),
                MemberDecl::Chain(_) => "chain".into(),
                MemberDecl::MetaBlock(_) => "meta".into(),
                MemberDecl::ForallConnect(_) => "forall_connect".into(),
                MemberDecl::ForallConstraint(_) => "forall_constraint".into(),
                // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
                MemberDecl::MatchArmDeclGroup(_) => "match_arm_decl_group".into(),
                // Produced by lower_function (task 3937).
                MemberDecl::Fn(f) => format!("fn:{}", f.name),
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "param:width",
                "param:height",
                "param:thickness",
                "param:fillet_radius",
                "param:hole_diameter",
                "let:volume",
                "constraint",
                "constraint",
                "constraint",
                "let:body",
            ]
        );
    }

    /// Helper to get structure members from bracket parse.
    fn bracket_members() -> Vec<MemberDecl> {
        let module = parse_bracket();
        match module.declarations.into_iter().next().unwrap() {
            Declaration::Structure(s) => s.members,
            _ => panic!("expected Structure"),
        }
    }

    #[test]
    fn quantity_literal_80mm() {
        let members = bracket_members();
        let width = match &members[0] {
            MemberDecl::Param(p) => p,
            _ => panic!("expected Param"),
        };
        assert_eq!(width.name, "width");
        match &width.default.as_ref().unwrap().kind {
            ExprKind::QuantityLiteral { value, unit } => {
                assert!((value - 80.0).abs() < f64::EPSILON);
                assert_eq!(unit, &UnitExpr::Unit("mm".to_string()));
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        }
    }

    #[test]
    fn number_literal_4() {
        // In `constraint thickness < width / 4`, the `4` is a number literal
        let members = bracket_members();
        // constraints[1] is `constraint thickness < width / 4`
        let constraint = match &members[7] {
            MemberDecl::Constraint(c) => c,
            _ => panic!("expected Constraint"),
        };
        // expr is `thickness < width / 4`
        match &constraint.expr.kind {
            ExprKind::BinOp { right, .. } => {
                // right is `width / 4`
                match &right.kind {
                    ExprKind::BinOp {
                        right: inner_right, ..
                    } => match &inner_right.kind {
                        ExprKind::NumberLiteral { value: v, .. } => {
                            assert!((v - 4.0).abs() < f64::EPSILON);
                        }
                        other => panic!("expected NumberLiteral(4), got {:?}", other),
                    },
                    other => panic!("expected BinOp, got {:?}", other),
                }
            }
            other => panic!("expected BinOp, got {:?}", other),
        }
    }

    #[test]
    fn function_call_box() {
        let members = bracket_members();
        // Last member: `let body = box(width, height, thickness)`
        let body = match &members[9] {
            MemberDecl::Let(l) => l,
            _ => panic!("expected Let"),
        };
        assert_eq!(body.name, "body");
        match &body.value.kind {
            ExprKind::FunctionCall { name, args } => {
                assert_eq!(name, "box");
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[0].kind, ExprKind::Ident(n) if n == "width"));
                assert!(matches!(&args[1].kind, ExprKind::Ident(n) if n == "height"));
                assert!(matches!(&args[2].kind, ExprKind::Ident(n) if n == "thickness"));
            }
            other => panic!("expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn binary_ops_left_associative() {
        let members = bracket_members();
        // `let volume = width * height * thickness`
        let volume = match &members[5] {
            MemberDecl::Let(l) => l,
            _ => panic!("expected Let"),
        };
        assert_eq!(volume.name, "volume");
        // Should be ((width * height) * thickness)
        match &volume.value.kind {
            ExprKind::BinOp { op, left, right } => {
                assert_eq!(op, "*");
                // right is "thickness"
                assert!(matches!(&right.kind, ExprKind::Ident(n) if n == "thickness"));
                // left is (width * height)
                match &left.kind {
                    ExprKind::BinOp {
                        op: inner_op,
                        left: ll,
                        right: lr,
                    } => {
                        assert_eq!(inner_op, "*");
                        assert!(matches!(&ll.kind, ExprKind::Ident(n) if n == "width"));
                        assert!(matches!(&lr.kind, ExprKind::Ident(n) if n == "height"));
                    }
                    other => panic!("expected inner BinOp, got {:?}", other),
                }
            }
            other => panic!("expected BinOp, got {:?}", other),
        }
    }

    #[test]
    fn comparison_with_quantity() {
        let members = bracket_members();
        // `constraint thickness > 2mm`
        let constraint = match &members[6] {
            MemberDecl::Constraint(c) => c,
            _ => panic!("expected Constraint"),
        };
        match &constraint.expr.kind {
            ExprKind::BinOp { op, left, right } => {
                assert_eq!(op, ">");
                assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "thickness"));
                match &right.kind {
                    ExprKind::QuantityLiteral { value, unit } => {
                        assert!((value - 2.0).abs() < f64::EPSILON);
                        assert_eq!(unit, &UnitExpr::Unit("mm".to_string()));
                    }
                    other => panic!("expected QuantityLiteral, got {:?}", other),
                }
            }
            other => panic!("expected BinOp, got {:?}", other),
        }
    }

    #[test]
    fn spans_are_valid_and_cover_source_text() {
        let source = reify_test_support::bracket_source();
        let module = parse(source, ModulePath::single("bracket"));

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };

        // Structure spans entire source
        assert_eq!(structure.span.start, 0);
        assert_eq!(structure.span.end as usize, source.len());

        // All member spans are non-empty, within source, and contain expected keywords
        for (i, m) in structure.members.iter().enumerate() {
            let span = match m {
                MemberDecl::Param(p) => p.span,
                MemberDecl::Let(l) => l.span,
                MemberDecl::Constraint(c) => c.span,
                MemberDecl::ConstraintInst(ci) => ci.span,
                MemberDecl::Sub(s) => s.span,
                MemberDecl::Minimize(m) => m.span,
                MemberDecl::Maximize(m) => m.span,
                MemberDecl::GuardedGroup(g) => g.span,
                MemberDecl::AssociatedType(a) => a.span,
                MemberDecl::Port(p) => p.span,
                MemberDecl::Connect(c) => c.span,
                MemberDecl::Chain(c) => c.span,
                MemberDecl::MetaBlock(m) => m.span,
                MemberDecl::ForallConnect(f) => f.span,
                MemberDecl::ForallConstraint(f) => f.span,
                // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
                MemberDecl::MatchArmDeclGroup(g) => g.span,
                // Produced by lower_function (task 3937).
                MemberDecl::Fn(f) => f.span,
            };
            assert!(span.start < span.end, "member {} span empty", i);
            assert!(
                (span.end as usize) <= source.len(),
                "member {} span overflows",
                i
            );

            let text = &source[span.start as usize..span.end as usize];
            match m {
                MemberDecl::Param(p) => {
                    assert!(
                        text.starts_with("param"),
                        "param member {} text: {:?}",
                        i,
                        text
                    );
                    assert!(text.contains(&p.name), "param {} name in text", i);
                }
                MemberDecl::Let(l) => {
                    assert!(text.starts_with("let"), "let member {} text: {:?}", i, text);
                    assert!(text.contains(&l.name), "let {} name in text", i);
                }
                MemberDecl::Constraint(_) => {
                    assert!(
                        text.starts_with("constraint"),
                        "constraint member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::Sub(s) => {
                    assert!(text.starts_with("sub"), "sub member {} text: {:?}", i, text);
                    assert!(text.contains(&s.name), "sub {} name in text", i);
                }
                MemberDecl::Minimize(_) => {
                    assert!(
                        text.starts_with("minimize"),
                        "minimize member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::Maximize(_) => {
                    assert!(
                        text.starts_with("maximize"),
                        "maximize member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::GuardedGroup(_) => {
                    assert!(
                        text.starts_with("where"),
                        "guarded_group member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::AssociatedType(a) => {
                    assert!(
                        text.starts_with("type"),
                        "associated_type member {} text: {:?}",
                        i,
                        text
                    );
                    assert!(text.contains(&a.name), "associated_type {} name in text", i);
                }
                MemberDecl::Port(p) => {
                    assert!(
                        text.starts_with("port"),
                        "port member {} text: {:?}",
                        i,
                        text
                    );
                    assert!(text.contains(&p.name), "port {} name in text", i);
                }
                MemberDecl::Connect(_) => {
                    assert!(
                        text.starts_with("connect"),
                        "connect member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::Chain(_) => {
                    assert!(
                        text.starts_with("chain"),
                        "chain member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::MetaBlock(_) => {
                    assert!(
                        text.starts_with("meta"),
                        "meta member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::ConstraintInst(ci) => {
                    assert!(
                        text.starts_with("constraint"),
                        "constraint_inst member {} text: {:?}",
                        i,
                        text
                    );
                    assert!(
                        text.contains(&ci.name),
                        "constraint_inst {} name in text",
                        i
                    );
                }
                MemberDecl::ForallConnect(_) => {
                    assert!(
                        text.starts_with("forall"),
                        "forall_connect member {} text: {:?}",
                        i,
                        text
                    );
                }
                MemberDecl::ForallConstraint(_) => {
                    assert!(
                        text.starts_with("forall"),
                        "forall_constraint member {} text: {:?}",
                        i,
                        text
                    );
                }
                // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
                MemberDecl::MatchArmDeclGroup(_) => {}
                // Produced by lower_function (task 3937).
                MemberDecl::Fn(f) => {
                    assert!(
                        text.starts_with("fn"),
                        "fn member {} text: {:?}",
                        i,
                        text
                    );
                    assert!(text.contains(&f.name), "fn {} name in text", i);
                }
            }
        }

        // Expression spans are valid
        if let MemberDecl::Param(p) = &structure.members[0] {
            let def_span = p.default.as_ref().unwrap().span;
            let def_text = &source[def_span.start as usize..def_span.end as usize];
            assert_eq!(def_text, "80mm", "width default text");

            let ty_span = p.type_expr.as_ref().unwrap().span;
            let ty_text = &source[ty_span.start as usize..ty_span.end as usize];
            assert_eq!(ty_text, "Scalar", "width type text");
        }
    }

    #[test]
    fn content_hashes_computed_from_source_text() {
        let source = reify_test_support::bracket_source();
        let module = parse(source, ModulePath::single("bracket"));

        // Module content hash = hash of entire source
        assert_eq!(
            module.content_hash,
            ContentHash::of_str(source),
            "module hash"
        );

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!("expected Structure"),
        };

        // Structure content hash = hash of structure node's source text (not entire file)
        assert_ne!(
            structure.content_hash,
            ContentHash(0),
            "structure hash should be non-zero"
        );

        // Each member content hash = hash of its source text slice
        for (i, m) in structure.members.iter().enumerate() {
            let (span, hash) = match m {
                MemberDecl::Param(p) => (p.span, p.content_hash),
                MemberDecl::Let(l) => (l.span, l.content_hash),
                MemberDecl::Constraint(c) => (c.span, c.content_hash),
                MemberDecl::ConstraintInst(ci) => (ci.span, ci.content_hash),
                MemberDecl::Sub(s) => (s.span, s.content_hash),
                MemberDecl::Minimize(m) => (m.span, m.content_hash),
                MemberDecl::Maximize(m) => (m.span, m.content_hash),
                MemberDecl::GuardedGroup(g) => (g.span, g.content_hash),
                MemberDecl::AssociatedType(a) => (a.span, a.content_hash),
                MemberDecl::Port(p) => (p.span, p.content_hash),
                MemberDecl::Connect(c) => (c.span, c.content_hash),
                MemberDecl::Chain(c) => (c.span, c.content_hash),
                MemberDecl::MetaBlock(m) => (m.span, m.content_hash),
                MemberDecl::ForallConnect(f) => (f.span, f.content_hash),
                MemberDecl::ForallConstraint(f) => (f.span, f.content_hash),
                // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
                MemberDecl::MatchArmDeclGroup(g) => (g.span, g.content_hash),
                // Produced by lower_function (task 3937).
                MemberDecl::Fn(f) => (f.span, f.content_hash),
            };
            let text = &source[span.start as usize..span.end as usize];
            assert_eq!(
                hash,
                ContentHash::of_str(text),
                "member {} hash from source text",
                i
            );
        }

        // All param hashes should be unique
        let param_hashes: Vec<ContentHash> = structure
            .members
            .iter()
            .filter_map(|m| match m {
                MemberDecl::Param(p) => Some(p.content_hash),
                _ => None,
            })
            .collect();
        for (i, h1) in param_hashes.iter().enumerate() {
            for (j, h2) in param_hashes.iter().enumerate() {
                if i != j {
                    assert_ne!(h1, h2, "params {} and {} have same hash", i, j);
                }
            }
        }
    }

    #[test]
    fn error_recovery_partial_parse() {
        let source = r#"structure Broken {
    param width: Scalar = 80mm
    param !!!invalid!!!
    param height: Scalar = 100mm
}"#;
        let module = parse(source, ModulePath::single("broken"));

        // Should have parse errors
        assert!(
            !module.errors.is_empty(),
            "expected errors for malformed input"
        );

        // Should also have recovered declarations
        assert!(
            !module.declarations.is_empty(),
            "expected partial declarations"
        );

        if let Declaration::Structure(s) = &module.declarations[0] {
            assert_eq!(s.name, "Broken");
            // Should have at least some valid members (width and/or height)
            let valid_params: Vec<_> = s
                .members
                .iter()
                .filter_map(|m| match m {
                    MemberDecl::Param(p) => Some(&p.name),
                    _ => None,
                })
                .collect();
            assert!(
                !valid_params.is_empty(),
                "expected at least some valid params, got none"
            );
        } else {
            panic!("expected Structure declaration");
        }
    }

    #[test]
    fn parse_deterministic() {
        // Parsing the same source twice produces identical output.
        let source = reify_test_support::bracket_source();
        let m1 = parse(source, ModulePath::single("bracket"));
        let m2 = parse(source, ModulePath::single("bracket"));

        assert_eq!(m1.content_hash, m2.content_hash);
        assert_eq!(m1.declarations.len(), m2.declarations.len());
        assert_eq!(m1.errors.len(), m2.errors.len());

        let s1 = match &m1.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!(),
        };
        let s2 = match &m2.declarations[0] {
            Declaration::Structure(s) => s,
            _ => panic!(),
        };

        assert_eq!(s1.name, s2.name);
        assert_eq!(s1.span, s2.span);
        assert_eq!(s1.content_hash, s2.content_hash);
        assert_eq!(s1.members.len(), s2.members.len());

        for (i, (m_a, m_b)) in s1.members.iter().zip(s2.members.iter()).enumerate() {
            let (hash_a, span_a) = match m_a {
                MemberDecl::Param(p) => (p.content_hash, p.span),
                MemberDecl::Let(l) => (l.content_hash, l.span),
                MemberDecl::Constraint(c) => (c.content_hash, c.span),
                MemberDecl::ConstraintInst(ci) => (ci.content_hash, ci.span),
                MemberDecl::Sub(s) => (s.content_hash, s.span),
                MemberDecl::Minimize(m) => (m.content_hash, m.span),
                MemberDecl::Maximize(m) => (m.content_hash, m.span),
                MemberDecl::GuardedGroup(g) => (g.content_hash, g.span),
                MemberDecl::AssociatedType(a) => (a.content_hash, a.span),
                MemberDecl::Port(p) => (p.content_hash, p.span),
                MemberDecl::Connect(c) => (c.content_hash, c.span),
                MemberDecl::Chain(c) => (c.content_hash, c.span),
                MemberDecl::MetaBlock(m) => (m.content_hash, m.span),
                MemberDecl::ForallConnect(f) => (f.content_hash, f.span),
                MemberDecl::ForallConstraint(f) => (f.content_hash, f.span),
                // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
                MemberDecl::MatchArmDeclGroup(g) => (g.content_hash, g.span),
                // Produced by lower_function (task 3937).
                MemberDecl::Fn(f) => (f.content_hash, f.span),
            };
            let (hash_b, span_b) = match m_b {
                MemberDecl::Param(p) => (p.content_hash, p.span),
                MemberDecl::Let(l) => (l.content_hash, l.span),
                MemberDecl::Constraint(c) => (c.content_hash, c.span),
                MemberDecl::ConstraintInst(ci) => (ci.content_hash, ci.span),
                MemberDecl::Sub(s) => (s.content_hash, s.span),
                MemberDecl::Minimize(m) => (m.content_hash, m.span),
                MemberDecl::Maximize(m) => (m.content_hash, m.span),
                MemberDecl::GuardedGroup(g) => (g.content_hash, g.span),
                MemberDecl::AssociatedType(a) => (a.content_hash, a.span),
                MemberDecl::Port(p) => (p.content_hash, p.span),
                MemberDecl::Connect(c) => (c.content_hash, c.span),
                MemberDecl::Chain(c) => (c.content_hash, c.span),
                MemberDecl::MetaBlock(m) => (m.content_hash, m.span),
                MemberDecl::ForallConnect(f) => (f.content_hash, f.span),
                MemberDecl::ForallConstraint(f) => (f.content_hash, f.span),
                // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
                MemberDecl::MatchArmDeclGroup(g) => (g.content_hash, g.span),
                // Produced by lower_function (task 3937).
                MemberDecl::Fn(f) => (f.content_hash, f.span),
            };
            assert_eq!(hash_a, hash_b, "member {} hash determinism", i);
            assert_eq!(span_a, span_b, "member {} span determinism", i);
        }
    }

    #[test]
    fn parse_minimize_declaration() {
        let source = r#"structure S {
    param volume: Scalar = 100mm
    minimize volume
}"#;
        let module = parse(source, ModulePath::single("test_min"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        // Should have 2 members: param + minimize
        assert_eq!(structure.members.len(), 2);

        match &structure.members[1] {
            MemberDecl::Minimize(m) => {
                assert!(matches!(&m.expr.kind, ExprKind::Ident(name) if name == "volume"));
            }
            other => panic!("expected Minimize, got {:?}", other),
        }
    }

    #[test]
    fn parse_maximize_declaration() {
        let source = r#"structure S {
    param thickness: Scalar = 5mm
    maximize thickness
}"#;
        let module = parse(source, ModulePath::single("test_max"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        assert_eq!(structure.members.len(), 2);

        match &structure.members[1] {
            MemberDecl::Maximize(m) => {
                assert!(matches!(&m.expr.kind, ExprKind::Ident(name) if name == "thickness"));
            }
            other => panic!("expected Maximize, got {:?}", other),
        }
    }

    #[test]
    fn parse_minimize_complex_expression() {
        let source = r#"structure S {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    minimize width * height
}"#;
        let module = parse(source, ModulePath::single("test_min_complex"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        match &structure.members[2] {
            MemberDecl::Minimize(m) => match &m.expr.kind {
                ExprKind::BinOp { op, .. } => assert_eq!(op, "*"),
                other => panic!("expected BinOp(*), got {:?}", other),
            },
            other => panic!("expected Minimize, got {:?}", other),
        }
    }

    #[test]
    fn parse_minimize_with_other_members() {
        let source = r#"structure S {
    param w: Scalar = 80mm
    param h: Scalar = 100mm
    let vol = w * h
    constraint w > 0mm
    minimize w
}"#;
        let module = parse(source, ModulePath::single("test_min_mixed"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        // 2 params + 1 let + 1 constraint + 1 minimize = 5 members
        assert_eq!(structure.members.len(), 5);

        // Verify minimize is present alongside other members
        assert!(
            structure
                .members
                .iter()
                .any(|m| matches!(m, MemberDecl::Minimize(_))),
            "should contain a Minimize member"
        );
        assert!(
            structure
                .members
                .iter()
                .any(|m| matches!(m, MemberDecl::Constraint(_))),
            "should contain a Constraint member"
        );
    }

    #[test]
    fn minimize_span_and_hash() {
        let source = r#"structure S {
    param x: Scalar = 5mm
    minimize x
}"#;
        let module = parse(source, ModulePath::single("test_min_span"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };

        match &structure.members[1] {
            MemberDecl::Minimize(m) => {
                // Span should cover the full "minimize x" text
                let text = &source[m.span.start as usize..m.span.end as usize];
                assert!(text.starts_with("minimize"), "span text: {:?}", text);
                assert!(
                    text.contains("x"),
                    "span text should contain 'x': {:?}",
                    text
                );

                // Content hash should match the source text of the node
                assert_eq!(
                    m.content_hash,
                    ContentHash::of_str(text),
                    "content_hash should match source text"
                );
            }
            other => panic!("expected Minimize, got {:?}", other),
        }
    }

    #[test]
    fn parse_enum_declaration() {
        let source = "enum Direction { In, Out, Bidi }\nstructure S { param x: Scalar = 5mm }";
        let module = parse(source, ModulePath::single("test_enum"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 2);

        match &module.declarations[0] {
            Declaration::Enum(e) => {
                assert_eq!(e.name, "Direction");
                assert_eq!(e.variants, vec!["In", "Out", "Bidi"]);
            }
            other => panic!("expected Enum, got {:?}", other),
        }
    }

    #[test]
    fn parse_enum_access_expression() {
        let source = "enum Direction { In, Out, Bidi }\nstructure S { let d = Direction.In }";
        let module = parse(source, ModulePath::single("test_enum_access"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let structure = module
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Structure(s) => Some(s),
                _ => None,
            })
            .expect("expected a structure");

        let let_decl = match &structure.members[0] {
            MemberDecl::Let(l) => l,
            other => panic!("expected Let, got {:?}", other),
        };
        assert_eq!(let_decl.name, "d");
        match &let_decl.value.kind {
            ExprKind::EnumAccess { type_name, variant } => {
                assert_eq!(type_name, "Direction");
                assert_eq!(variant, "In");
            }
            other => panic!("expected EnumAccess, got {:?}", other),
        }
    }

    #[test]
    fn parse_enum_missing_name_is_error() {
        let source = "enum { }";
        let module = parse(source, ModulePath::single("test_enum_err"));
        assert!(
            !module.errors.is_empty(),
            "expected parse errors for malformed enum"
        );
    }

    // ── parse_with_prelude_enums (task 2525) ────────────────────────────────

    /// Helper: locate the first `EnumAccess` expression in a parsed module's
    /// structure declarations.  Returns the matched `(type_name, variant)`
    /// pair, or `None` if no `EnumAccess` is present.
    ///
    /// NOTE (task 2559): a shared `reify_test_support::visit_structure_member_root_exprs`
    /// helper exists but cannot be called from inside `reify-syntax`'s own
    /// `#[cfg(test)]` module. The `reify-syntax` ↔ `reify-test-support`
    /// dev-dep cycle causes `cargo test -p reify-syntax` to compile
    /// `reify-syntax` twice (once as the test binary with `cfg(test)`, once
    /// as the library that `reify-test-support` links against). The two
    /// `ParsedModule`/`Expr` instantiations are nominally distinct, so a
    /// `visit_structure_member_root_exprs(&module, ...)` call from here fails to
    /// type-check (E0308: "multiple different versions of crate
    /// `reify_syntax` in the dependency graph"). Out-of-crate call sites
    /// (e.g. `crates/reify-compiler/tests/parse_with_stdlib_tests.rs`) DO
    /// use the shared helper.
    fn find_first_enum_access(module: &ParsedModule) -> Option<(String, String)> {
        for decl in &module.declarations {
            if let Declaration::Structure(s) = decl {
                for member in &s.members {
                    if let MemberDecl::Let(l) = member
                        && let ExprKind::EnumAccess { type_name, variant } = &l.value.kind
                    {
                        return Some((type_name.clone(), variant.clone()));
                    }
                }
            }
        }
        None
    }

    /// (a) When `parse_with_prelude_enums` is given an enum name that is NOT
    /// declared in the source, `Foo.Bar` must lower to `EnumAccess { type_name: "Foo", variant: "Bar" }`
    /// rather than `MemberAccess { object: Ident("Foo"), member: "Bar" }`.
    /// This is the core behavior change motivated by task 2525: prelude enums
    /// must participate in EnumAccess disambiguation.
    #[test]
    fn parse_with_prelude_enums_resolves_prelude_only_enum() {
        let source = "structure S { let v = Foo.Bar }";
        let module = parse_with_prelude_enums(
            source,
            ModulePath::single("test_prelude_enum"),
            &["Foo"],
        );
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let (type_name, variant) = find_first_enum_access(&module)
            .expect("expected at least one EnumAccess in parsed module");
        assert_eq!(type_name, "Foo");
        assert_eq!(variant, "Bar");
    }

    /// (b) When the same enum name appears in BOTH `prelude_enum_names` and
    /// the source's own `enum_declaration`, no parse error fires (the parser
    /// does not policed prelude/source name overlap), and the disambiguation
    /// still resolves `Foo.Bar` to `EnumAccess`.  This pins the contract that
    /// duplicate-prelude/source enum names are tolerated at parse time and
    /// left to downstream name resolution to handle.
    #[test]
    fn parse_with_prelude_enums_dedupes_overlap_with_source_enum() {
        let source = "enum Foo { Bar, Baz }\nstructure S { let v = Foo.Bar }";
        let module = parse_with_prelude_enums(
            source,
            ModulePath::single("test_prelude_overlap"),
            &["Foo"],
        );
        assert!(
            module.errors.is_empty(),
            "parse errors should be empty even when prelude and source share an enum name: {:?}",
            module.errors
        );

        let (type_name, variant) = find_first_enum_access(&module)
            .expect("expected at least one EnumAccess in parsed module");
        assert_eq!(type_name, "Foo");
        assert_eq!(variant, "Bar");
    }

    /// (c) `parse_with_prelude_enums(source, path, &[])` must be
    /// observationally equivalent to `parse(source, path)`.  This is a
    /// regression guard that pins the empty-prelude case so the wrapper never
    /// drifts away from the unparameterized `parse` behavior.
    #[test]
    fn parse_with_prelude_enums_empty_slice_equivalent_to_parse() {
        let source = "enum Direction { In, Out, Bidi }\nstructure S { let d = Direction.In }";
        let path = ModulePath::single("test_empty_prelude");

        let from_parse = parse(source, path.clone());
        let from_prelude = parse_with_prelude_enums(source, path, &[]);

        // Same parse-error count and same content_hash captures observational
        // equivalence at the `ParsedModule` level.
        assert_eq!(
            from_parse.errors.len(),
            from_prelude.errors.len(),
            "empty-slice prelude must produce the same parse error count as parse()"
        );
        assert_eq!(
            from_parse.content_hash, from_prelude.content_hash,
            "empty-slice prelude must produce the same content_hash as parse()"
        );
        assert_eq!(
            from_parse.declarations.len(),
            from_prelude.declarations.len(),
            "empty-slice prelude must produce the same declaration count as parse()"
        );

        // Both must locate the same `Direction.In` EnumAccess.
        let from_parse_access = find_first_enum_access(&from_parse).expect("parse() EnumAccess");
        let from_prelude_access =
            find_first_enum_access(&from_prelude).expect("parse_with_prelude_enums() EnumAccess");
        assert_eq!(from_parse_access, from_prelude_access);
    }

    /// (d) Regression guard for the `HashSet<&'a str>` borrow-through contract
    /// (task 2558).  Pins two invariants:
    ///
    /// 1. Functional correctness when the same `static` prelude slice is reused
    ///    across two consecutive `parse_with_prelude_enums` calls
    ///    (lifetime-mixing regression: both calls must resolve correctly without
    ///    interference from a prior call's internal state).
    /// 2. Mixed-source resolution: in the second call a source-declared enum
    ///    (`SourceEnum`) and a prelude-supplied enum (`PreludeEnumB`) must BOTH
    ///    lower to `EnumAccess` in the same parse.
    ///
    /// Note: the `&[&'static str]` API-surface constraint is enforced by the
    /// compiler — a non-`'static` borrow at a call site will not compile.  This
    /// test does not add dedicated coverage for that property (the compiler is
    /// the authoritative check).  The no-allocation guarantee is a manual
    /// profiling check (per task description), not encoded here.
    #[test]
    fn parse_with_prelude_enums_borrows_static_names_across_calls() {
        static PRELUDE: &[&str] = &["PreludeEnumA", "PreludeEnumB"];

        // First call — prelude-only enum (no source enum declarations).
        let source1 = "structure S1 { let v = PreludeEnumA.X }";
        let module1 = parse_with_prelude_enums(
            source1,
            ModulePath::single("test_borrow_call1"),
            PRELUDE,
        );
        assert!(
            module1.errors.is_empty(),
            "call 1 parse errors: {:?}",
            module1.errors
        );
        let (type1, variant1) =
            find_first_enum_access(&module1).expect("call 1: expected EnumAccess");
        assert_eq!(type1, "PreludeEnumA");
        assert_eq!(variant1, "X");

        // Second call — source-declared enum + prelude enum, same PRELUDE slice.
        // Both PreludeEnumB.Z and SourceEnum.Y must resolve to EnumAccess.
        let source2 =
            "enum SourceEnum { Y }\nstructure S2 { let v = PreludeEnumB.Z\n let w = SourceEnum.Y }";
        let module2 = parse_with_prelude_enums(
            source2,
            ModulePath::single("test_borrow_call2"),
            PRELUDE,
        );
        assert!(
            module2.errors.is_empty(),
            "call 2 parse errors: {:?}",
            module2.errors
        );

        // Collect all EnumAccess let-decl values from S2.
        let mut accesses: Vec<(String, String)> = Vec::new();
        for decl in &module2.declarations {
            if let Declaration::Structure(s) = decl {
                for member in &s.members {
                    if let MemberDecl::Let(l) = member
                        && let ExprKind::EnumAccess { type_name, variant } = &l.value.kind
                    {
                        accesses.push((type_name.clone(), variant.clone()));
                    }
                }
            }
        }
        assert!(
            accesses.contains(&("PreludeEnumB".to_string(), "Z".to_string())),
            "expected PreludeEnumB.Z → EnumAccess; got: {:?}",
            accesses
        );
        assert!(
            accesses.contains(&("SourceEnum".to_string(), "Y".to_string())),
            "expected SourceEnum.Y → EnumAccess; got: {:?}",
            accesses
        );
    }

    #[test]
    fn tree_sitter_parses_bracket_source_without_errors() {
        let source = reify_test_support::bracket_source();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");

        let tree = parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        assert_eq!(root.kind(), "source_file");
        assert_eq!(
            count_errors(root),
            0,
            "Expected zero ERROR nodes, got tree:\n{}",
            root.to_sexp()
        );
    }

    // ── Collection literal tests ──────────────────────────

    /// Helper: parse a source string wrapping an expression in a structure let,
    /// and return the ExprKind of the let's value.
    fn parse_let_expr(source: &str) -> ExprKind {
        let module = parse(source, ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };
        let let_decl = match &structure.members[0] {
            MemberDecl::Let(l) => l,
            other => panic!("expected Let, got {:?}", other),
        };
        let_decl.value.kind.clone()
    }

    #[test]
    fn parse_list_literal_three_elements() {
        let kind = parse_let_expr("structure S { let x = [1, 2, 3] }");
        match kind {
            ExprKind::ListLiteral(elems) => {
                assert_eq!(elems.len(), 3);
                assert!(
                    matches!(&elems[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(&elems[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(&elems[2].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
                );
            }
            other => panic!("expected ListLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_list_literal_empty() {
        let kind = parse_let_expr("structure S { let x = [] }");
        match kind {
            ExprKind::ListLiteral(elems) => {
                assert_eq!(elems.len(), 0);
            }
            other => panic!("expected ListLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_set_literal_three_elements() {
        let kind = parse_let_expr("structure S { let x = set{1, 2, 3} }");
        match kind {
            ExprKind::SetLiteral(elems) => {
                assert_eq!(elems.len(), 3);
                assert!(
                    matches!(&elems[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(&elems[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(&elems[2].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
                );
            }
            other => panic!("expected SetLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_set_literal_empty() {
        let kind = parse_let_expr("structure S { let x = set{} }");
        match kind {
            ExprKind::SetLiteral(elems) => {
                assert_eq!(elems.len(), 0);
            }
            other => panic!("expected SetLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_literal_two_entries() {
        let kind = parse_let_expr(r#"structure S { let x = map{"a" => 1, "b" => 2} }"#);
        match kind {
            ExprKind::MapLiteral(entries) => {
                assert_eq!(entries.len(), 2);
                assert!(matches!(&entries[0].0.kind, ExprKind::StringLiteral(s) if s == "a"));
                assert!(
                    matches!(&entries[0].1.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
                );
                assert!(matches!(&entries[1].0.kind, ExprKind::StringLiteral(s) if s == "b"));
                assert!(
                    matches!(&entries[1].1.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                );
            }
            other => panic!("expected MapLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_literal_empty() {
        let kind = parse_let_expr("structure S { let x = map{} }");
        match kind {
            ExprKind::MapLiteral(entries) => {
                assert_eq!(entries.len(), 0);
            }
            other => panic!("expected MapLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_index_access_number() {
        let kind = parse_let_expr("structure S { let x = items[0] }");
        match kind {
            ExprKind::IndexAccess { object, index } => {
                assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "items"));
                assert!(
                    matches!(&index.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 0.0).abs() < f64::EPSILON)
                );
            }
            other => panic!("expected IndexAccess, got {:?}", other),
        }
    }

    #[test]
    fn parse_index_access_string_key() {
        let kind = parse_let_expr(r#"structure S { let x = m["key"] }"#);
        match kind {
            ExprKind::IndexAccess { object, index } => {
                assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "m"));
                assert!(matches!(&index.kind, ExprKind::StringLiteral(s) if s == "key"));
            }
            other => panic!("expected IndexAccess, got {:?}", other),
        }
    }

    #[test]
    fn parse_nested_list_literals() {
        let kind = parse_let_expr("structure S { let x = [[1, 2], [3, 4]] }");
        match kind {
            ExprKind::ListLiteral(outer) => {
                assert_eq!(outer.len(), 2);
                match &outer[0].kind {
                    ExprKind::ListLiteral(inner) => {
                        assert_eq!(inner.len(), 2);
                        assert!(
                            matches!(&inner[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
                        );
                        assert!(
                            matches!(&inner[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                        );
                    }
                    other => panic!("expected inner ListLiteral, got {:?}", other),
                }
                match &outer[1].kind {
                    ExprKind::ListLiteral(inner) => {
                        assert_eq!(inner.len(), 2);
                        assert!(
                            matches!(&inner[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
                        );
                        assert!(
                            matches!(&inner[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 4.0).abs() < f64::EPSILON)
                        );
                    }
                    other => panic!("expected inner ListLiteral, got {:?}", other),
                }
            }
            other => panic!("expected outer ListLiteral, got {:?}", other),
        }
    }

    #[test]
    fn parse_collection_in_let_context() {
        let source = "structure S { let x = [1, 2, 3] }";
        let module = parse(source, ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 1);
        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };
        assert_eq!(structure.members.len(), 1);
        let let_decl = match &structure.members[0] {
            MemberDecl::Let(l) => l,
            other => panic!("expected Let, got {:?}", other),
        };
        assert_eq!(let_decl.name, "x");
        assert!(matches!(&let_decl.value.kind, ExprKind::ListLiteral(elems) if elems.len() == 3));
    }

    #[test]
    fn parse_collections_no_regression_on_bracket() {
        let module = parse_bracket();
        assert!(
            module.errors.is_empty(),
            "expected no errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 1);
        let structure = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got {:?}", other),
        };
        assert_eq!(structure.name, "Bracket");
        assert_eq!(
            structure.members.len(),
            10,
            "expected 10 members (5 params, 2 lets, 3 constraints)"
        );
        let params = structure
            .members
            .iter()
            .filter(|m| matches!(m, MemberDecl::Param(_)))
            .count();
        let lets = structure
            .members
            .iter()
            .filter(|m| matches!(m, MemberDecl::Let(_)))
            .count();
        let constraints = structure
            .members
            .iter()
            .filter(|m| matches!(m, MemberDecl::Constraint(_)))
            .count();
        assert_eq!(params, 5, "expected 5 params");
        assert_eq!(lets, 2, "expected 2 lets");
        assert_eq!(constraints, 3, "expected 3 constraints");
    }

    // ── Function definition tests ─────────────────────────────────

    #[test]
    fn parse_simple_function_definition() {
        let source = "fn area(w: Scalar, h: Scalar) -> Scalar { w * h }";
        let module = parse(source, ModulePath::single("test_fn"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 1);

        let f = match &module.declarations[0] {
            Declaration::Function(f) => f,
            other => panic!("expected Function, got {:?}", other),
        };
        assert_eq!(f.name, "area");
        assert!(!f.is_pub);
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "w");
        assert!(
            matches!(&f.params[0].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Scalar")
        );
        assert_eq!(f.params[1].name, "h");
        assert!(
            matches!(&f.params[1].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Scalar")
        );
        assert!(f.return_type.is_some());
        assert!(
            matches!(&f.return_type.as_ref().unwrap().kind, TypeExprKind::Named { name, .. } if name == "Scalar")
        );
        assert!(f.body.as_ref().unwrap().let_bindings.is_empty());
        assert!(matches!(&f.body.as_ref().unwrap().result_expr.kind, ExprKind::BinOp { op, .. } if op == "*"));
    }

    #[test]
    fn parse_pub_function_with_conditional() {
        let source = "pub fn clamp(x: Real, lo: Real, hi: Real) -> Real { if x < lo then lo else if x > hi then hi else x }";
        let module = parse(source, ModulePath::single("test_pub_fn"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 1);

        let f = match &module.declarations[0] {
            Declaration::Function(f) => f,
            other => panic!("expected Function, got {:?}", other),
        };
        assert!(f.is_pub);
        assert_eq!(f.name, "clamp");
        assert_eq!(f.params.len(), 3);
        assert_eq!(f.params[0].name, "x");
        assert!(
            matches!(&f.params[0].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Real")
        );
        assert_eq!(f.params[1].name, "lo");
        assert_eq!(f.params[2].name, "hi");
        assert!(f.return_type.is_some());
        assert!(
            matches!(&f.return_type.as_ref().unwrap().kind, TypeExprKind::Named { name, .. } if name == "Real")
        );
        assert!(matches!(
            &f.body.as_ref().unwrap().result_expr.kind,
            ExprKind::Conditional { .. }
        ));
    }

    #[test]
    fn parse_function_with_let_bindings() {
        let source = "fn f(x: Real) -> Real { let y = x * 2; y + 1 }";
        let module = parse(source, ModulePath::single("test_fn_let"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        assert_eq!(module.declarations.len(), 1);

        let f = match &module.declarations[0] {
            Declaration::Function(f) => f,
            other => panic!("expected Function, got {:?}", other),
        };
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.body.as_ref().unwrap().let_bindings.len(), 1);
        assert_eq!(f.body.as_ref().unwrap().let_bindings[0].name, "y");
        assert!(
            matches!(&f.body.as_ref().unwrap().let_bindings[0].value.kind, ExprKind::BinOp { op, .. } if op == "*")
        );
        assert!(matches!(&f.body.as_ref().unwrap().result_expr.kind, ExprKind::BinOp { op, .. } if op == "+"));
    }

    #[test]
    fn parse_function_with_type_parameters() {
        let source = "fn identity<T>(x: T) -> T { x }";
        let module = parse(source, ModulePath::single("test_fn_tp"));
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );

        let f = match &module.declarations[0] {
            Declaration::Function(f) => f,
            other => panic!("expected Function, got {:?}", other),
        };
        assert_eq!(f.type_params.len(), 1);
        assert_eq!(f.type_params[0].name, "T");
        assert!(f.type_params[0].bounds.is_empty());

        // Also test with bounds
        let source2 = "fn add<T: Numeric>(a: T, b: T) -> T { a + b }";
        let module2 = parse(source2, ModulePath::single("test_fn_tp2"));
        assert!(
            module2.errors.is_empty(),
            "parse errors: {:?}",
            module2.errors
        );

        let f2 = match &module2.declarations[0] {
            Declaration::Function(f) => f,
            other => panic!("expected Function, got {:?}", other),
        };
        assert_eq!(f2.type_params.len(), 1);
        assert_eq!(f2.type_params[0].name, "T");
        assert_eq!(f2.type_params[0].bounds, vec!["Numeric"]);
    }

    // ── Ad-hoc selector tests ─────────────────────────────

    #[test]
    fn parse_ad_hoc_selector_basic() {
        let kind = parse_let_expr(r#"structure S { let x = port @ face("top") }"#);
        match kind {
            ExprKind::AdHocSelector {
                base,
                selector,
                args,
            } => {
                assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "port"));
                assert_eq!(selector, "face");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0].kind, ExprKind::StringLiteral(s) if s == "top"));
            }
            other => panic!("expected AdHocSelector, got {:?}", other),
        }
    }

    #[test]
    fn parse_ad_hoc_selector_no_args() {
        let kind = parse_let_expr("structure S { let x = port @ default() }");
        match kind {
            ExprKind::AdHocSelector {
                base,
                selector,
                args,
            } => {
                assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "port"));
                assert_eq!(selector, "default");
                assert_eq!(args.len(), 0);
            }
            other => panic!("expected AdHocSelector, got {:?}", other),
        }
    }

    #[test]
    fn parse_ad_hoc_selector_multiple_args() {
        let kind = parse_let_expr("structure S { let x = port @ point(1, 2, 3) }");
        match kind {
            ExprKind::AdHocSelector {
                base,
                selector,
                args,
            } => {
                assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "port"));
                assert_eq!(selector, "point");
                assert_eq!(args.len(), 3);
                assert!(
                    matches!(&args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(&args[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(&args[2].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
                );
            }
            other => panic!("expected AdHocSelector, got {:?}", other),
        }
    }

    // ── lower_connect_body direct tests ─────────────────────────────
    //
    // These tests call lower_connect_body directly, bypassing the
    // check_and_lower! guard that normally preempts body-level
    // diagnostics when has_error() propagates to the connect_statement.

    /// Helper: parse source with tree-sitter and find the first node of a given kind.
    fn find_node_by_kind<'a>(
        node: tree_sitter::Node<'a>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_node_by_kind(child, kind) {
                return Some(found);
            }
        }
        None
    }

    /// Generic helper: parse source, find the first node of `kind`, run `lower_fn`
    /// on it via a fresh `Lowering`, and return collected errors.
    ///
    /// The closure pattern avoids lifetime issues: `tree_sitter::Node` borrows from
    /// `Tree`, so both must live inside the same scope — the closure receives them
    /// without the caller needing to hold the `Tree`.
    fn lower_node_directly<F>(source: &str, kind: &str, lower_fn: F) -> Vec<ParseError>
    where
        F: FnOnce(&mut Lowering, tree_sitter::Node),
    {
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();
        assert!(
            !root.has_error(),
            "source '{}' should parse without errors — grammar regression?",
            source,
        );

        let node = find_node_by_kind(root, kind)
            .unwrap_or_else(|| panic!("no {kind} node found in parse tree"));

        let mut lowering = Lowering::new(source);
        lower_fn(&mut lowering, node);
        lowering.errors.into_inner()
    }

    /// Like `lower_node_directly`, but skips the clean-parse assertion.
    /// Use for tests that deliberately feed malformed source to exercise
    /// error-handling code paths.
    fn lower_node_with_errors<F>(source: &str, kind: &str, lower_fn: F) -> Vec<ParseError>
    where
        F: FnOnce(&mut Lowering, tree_sitter::Node),
    {
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        let node = find_node_by_kind(root, kind)
            .unwrap_or_else(|| panic!("no {kind} node found in parse tree"));

        let mut lowering = Lowering::new(source);
        lower_fn(&mut lowering, node);
        lowering.errors.into_inner()
    }

    /// Helper: parse source, find the connect_body node, call lower_connect_body
    /// directly (bypassing check_and_lower!), and return the errors.
    fn lower_body_directly(source: &str) -> Vec<ParseError> {
        lower_node_directly(source, "connect_body", |l, n| {
            l.lower_connect_body(n);
        })
    }

    /// Like `lower_body_directly`, but skips the clean-parse assertion.
    fn lower_body_with_errors(source: &str) -> Vec<ParseError> {
        lower_node_with_errors(source, "connect_body", |l, n| {
            l.lower_connect_body(n);
        })
    }

    #[test]
    #[should_panic(expected = "should parse without errors")]
    fn lower_node_directly_rejects_source_with_parse_errors() {
        // Deliberately broken source: `{ >= }` produces parse errors.
        // lower_node_directly should panic because root.has_error() is true.
        lower_body_directly("structure S { port a : out T  port b : in T  connect a -> b { >= } }");
    }

    #[test]
    fn lower_connect_body_error_node_emits_diagnostic() {
        // `{ >= }` produces an ERROR child inside connect_body.
        // When lower_connect_body is called directly, the ERROR arm fires.
        let errors = lower_body_with_errors(
            "structure S { port a : out T  port b : in T  connect a -> b { >= } }",
        );
        assert!(
            !errors.is_empty(),
            "expected body-level diagnostic for ERROR node, got none"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("syntax error in connect body")),
            "expected 'syntax error in connect body', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_connect_body_malformed_param_emits_diagnostic() {
        // `{ grade = }` produces a connect_param_assignment with has_error().
        // When lower_connect_body is called directly, the has_error() guard fires.
        let errors = lower_body_with_errors(
            "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = } }",
        );
        assert!(
            !errors.is_empty(),
            "expected body-level diagnostic for malformed param, got none"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("connect parameter")),
            "expected error mentioning 'connect parameter', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_connect_body_malformed_mapping_emits_diagnostic() {
        // `{ shaft -> }` produces a port_mapping with has_error().
        // When lower_connect_body is called directly, the has_error() guard fires.
        let errors = lower_body_with_errors(
            "structure S { port a : out T  port b : in T  connect a -> b { shaft -> } }",
        );
        assert!(
            !errors.is_empty(),
            "expected body-level diagnostic for malformed mapping, got none"
        );
        assert!(
            errors.iter().any(|e| e.message.contains("port mapping")),
            "expected error mentioning 'port mapping', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_connect_body_extras_not_flagged() {
        // Comments are tree-sitter extras — they must NOT trigger the catch-all
        // diagnostic. The source is syntactically valid, so zero errors is the
        // correct assertion (not just "no 'unexpected' errors").
        let errors = lower_body_directly(
            "structure S { port a : out T  port b : in T  connect a -> b { /* comment */ grade = 8.8 }  }",
        );
        assert!(
            errors.is_empty(),
            "expected no errors for syntactically valid connect body with comment, got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_connect_body_anonymous_tokens_not_flagged() {
        // An empty connect body `{ }` has only anonymous tokens (braces).
        // The named-children iteration must skip them without producing errors.
        let errors = lower_body_directly(
            "structure S { port a : out T  port b : in T  connect a -> b { } }",
        );
        assert!(
            errors.is_empty(),
            "expected no errors for empty connect body (anonymous tokens only), got: {:?}",
            errors
        );
    }

    /// Deliberately passes a `constraint_definition` node to `lower_connect_body`
    /// to exercise the catch-all branch. The constraint_definition has 3 named
    /// children (identifier, param_declaration, constraint_def_predicate), none of
    /// which match any connect_body arm — so the catch-all should fire for each.
    #[test]
    fn lower_connect_body_catch_all_emits_for_unexpected_named_children() {
        let source = "constraint def Eq { param x: Scalar  x > 0 }";
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        assert!(
            !root.has_error(),
            "source should parse without errors — grammar regression?"
        );

        let Some(constraint_node) = find_node_by_kind(root, "constraint_definition") else {
            panic!("no constraint_definition node found in parse tree — grammar regression?");
        };

        let mut lowering = Lowering::new(source);
        lowering.lower_connect_body(constraint_node);
        let errors = lowering.errors.borrow();
        assert!(
            errors.len() >= 3,
            "expected at least 3 diagnostics (one per named child: identifier, \
             param_declaration, constraint_def_predicate), got {}: {:?}",
            errors.len(),
            errors
        );
        assert!(
            errors.iter().any(|e| e.message.contains("unexpected")),
            "expected at least one error containing 'unexpected', got: {:?}",
            errors
        );
    }

    // ── Port body defensive catch-all tests ────────────────────

    /// Helper: parse source, find the port_body node, call lower_port_body
    /// directly (bypassing check_and_lower!), and return the errors.
    fn lower_port_body_directly(source: &str) -> Vec<ParseError> {
        lower_node_directly(source, "port_body", |l, n| {
            l.lower_port_body(n);
        })
    }

    /// Like `lower_port_body_directly`, but skips the clean-parse assertion.
    fn lower_port_body_with_errors(source: &str) -> Vec<ParseError> {
        lower_node_with_errors(source, "port_body", |l, n| {
            l.lower_port_body(n);
        })
    }

    #[test]
    fn lower_port_body_error_node_emits_diagnostic() {
        // `{ >= }` produces an ERROR child inside port_body.
        // When lower_port_body is called directly, the ERROR arm should fire.
        let errors = lower_port_body_with_errors("structure S { port a : in T { >= } }");
        assert!(
            !errors.is_empty(),
            "expected body-level diagnostic for ERROR node, got none"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("syntax error in port body")),
            "expected 'syntax error in port body', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_port_body_catch_all_emits_for_unexpected_named_children() {
        // Pass a constraint_definition node to lower_port_body. Its named
        // children (identifier, param_declaration, constraint_def_predicate)
        // don't match any port_body arm and should hit the catch-all.
        let source = "constraint def Eq { param x: Scalar  x > 0 }";
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        assert!(
            !root.has_error(),
            "source should parse without errors — grammar regression?"
        );

        let Some(constraint_node) = find_node_by_kind(root, "constraint_definition") else {
            panic!("no constraint_definition node found in parse tree — grammar regression?");
        };

        let mut lowering = Lowering::new(source);
        lowering.lower_port_body(constraint_node);
        let errors = lowering.errors.borrow();
        assert!(
            errors.len() >= 2,
            "expected at least 2 diagnostics (identifier and constraint_def_predicate \
             are unexpected in port body; param_declaration is handled), got {}: {:?}",
            errors.len(),
            errors
        );
        assert!(
            errors.iter().any(|e| e.message.contains("unexpected")),
            "expected at least one error containing 'unexpected', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_port_body_extras_not_flagged() {
        // Comments are tree-sitter extras — they must NOT trigger the catch-all
        // diagnostic. The source is syntactically valid, so zero errors is the
        // correct assertion (not just "no 'unexpected' errors").
        let errors = lower_port_body_directly(
            "structure S { port a : in T { /* comment */ param x: Scalar = 1 } }",
        );
        assert!(
            errors.is_empty(),
            "expected no errors for syntactically valid port body with comment, got: {:?}",
            errors
        );
    }

    // ── Constraint def defensive catch-all tests ───────────────

    /// Helper: parse source, find the constraint_definition node, call
    /// lower_constraint_def directly, and return the errors.
    fn lower_constraint_def_directly(source: &str) -> Vec<ParseError> {
        lower_node_directly(source, "constraint_definition", |l, n| {
            l.lower_constraint_def(n);
        })
    }

    #[test]
    fn lower_constraint_def_catch_all_emits_for_unexpected_named_children() {
        // Pass a structure_definition node to lower_constraint_def. Its named
        // children (sub_declaration, port_declaration, connect_declaration)
        // don't match constraint_def arms and should hit the catch-all.
        // We use structure_definition because it has a "name" field (required
        // by lower_constraint_def) and body children outside constraint scope.
        let source = "structure S { port a : in T { param x: Scalar = 1 }  sub b = T() }";
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        assert!(
            !root.has_error(),
            "source should parse without errors — grammar regression?"
        );

        let Some(struct_node) = find_node_by_kind(root, "structure_definition") else {
            panic!("no structure_definition node found in parse tree — grammar regression?");
        };

        let mut lowering = Lowering::new(source);
        lowering.lower_constraint_def(struct_node);
        let errors = lowering.errors.borrow();
        assert!(
            errors.len() >= 2,
            "expected at least 2 diagnostics (port_declaration, sub_declaration \
             at minimum), got {}: {:?}",
            errors.len(),
            errors
        );
        assert!(
            errors.iter().any(|e| e.message.contains("unexpected")),
            "expected at least one error containing 'unexpected', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_constraint_def_extras_not_flagged() {
        // Comments are tree-sitter extras — they must NOT trigger the catch-all
        // diagnostic. The source is syntactically valid, so zero errors is the
        // correct assertion (not just "no 'unexpected' errors").
        let errors = lower_constraint_def_directly(
            "constraint def Eq { /* comment */ param x: Scalar  x > 0 }",
        );
        assert!(
            errors.is_empty(),
            "expected no errors for syntactically valid constraint def with comment, got: {:?}",
            errors
        );
    }

    // ── Source file defensive catch-all tests ──────────────────

    #[test]
    fn lower_source_file_catch_all_emits_for_unexpected_named_children() {
        // Pass a structure_definition node to lower_source_file. Its named
        // children (identifier, param_declaration, port_declaration, etc.)
        // don't match any top-level declaration kind and should hit the catch-all.
        let source = "structure S { param x: Scalar = 1  port a : in T { param y: Scalar = 2 } }";
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        let struct_node = find_node_by_kind(root, "structure_definition")
            .expect("no structure_definition node found in parse tree");

        let mut lowering = Lowering::new(source);
        lowering.lower_source_file(struct_node);
        let errors = lowering.errors.borrow();
        assert!(
            !errors.is_empty(),
            "expected diagnostics for unexpected named children in source file catch-all, got none"
        );
        assert!(
            errors.iter().any(|e| e.message.contains("unexpected")),
            "expected at least one error containing 'unexpected', got: {:?}",
            errors
        );
    }

    #[test]
    fn lower_source_file_extras_not_flagged() {
        // Comments are tree-sitter extras — they must NOT trigger the catch-all
        // diagnostic. Verify that a source file with a block comment before a
        // valid structure produces no errors mentioning "unexpected".
        let source = "/* comment */\nstructure S { param x: Scalar = 1 }";
        let module = parse(source, ModulePath::single("test"));
        assert!(
            !module
                .errors
                .iter()
                .any(|e| e.message.contains("unexpected")),
            "expected no 'unexpected' errors for comment extras, got: {:?}",
            module.errors
        );
    }

    // ── Doc comment extraction tests ─────────────────────────

    #[test]
    fn doc_comment_on_structure_is_extracted() {
        let src = "/// A bracket for mounting.\nstructure Bracket {\n  param w: Scalar = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("A bracket for mounting."));
    }

    #[test]
    fn multi_line_doc_comment_joined() {
        let src = "/// Line one.\n/// Line two.\nstructure S {\n  param x: Scalar = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("Line one.\nLine two."));
    }

    #[test]
    fn no_doc_comment_yields_none() {
        let src = "structure S {\n  param x: Scalar = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert!(decl.doc.is_none());
    }

    #[test]
    fn regular_comment_not_treated_as_doc() {
        let src = "// Just a comment\nstructure S {\n  param x: Scalar = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert!(
            decl.doc.is_none(),
            "regular // comment should not be a doc comment"
        );
    }

    #[test]
    fn doc_comment_on_fn_is_extracted() {
        let src = "/// Compute area.\nfn area(w: Scalar, h: Scalar) -> Scalar { w * h }";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Function(f) => f,
            other => panic!("expected Function, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("Compute area."));
    }

    #[test]
    fn doc_comment_on_enum_is_extracted() {
        let src = "/// Direction enum.\nenum Dir { In, Out }";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Enum(e) => e,
            other => panic!("expected Enum, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("Direction enum."));
    }

    #[test]
    fn doc_comment_on_trait_is_extracted() {
        let src = "/// A rigid body.\ntrait Rigid {\n  param mass: Scalar\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Trait(t) => t,
            other => panic!("expected Trait, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("A rigid body."));
    }

    // PRD v0.6 D5: single-sided range lowering (task 3914 / ζ).
    // Grammar (task 3911) names the prefix fields `op` and `bound`.
    // Discriminator: two-sided has `lower`/`upper` fields; single-sided has `op`/`bound` fields.
    #[test]
    fn single_sided_range_gt_lower_exclusive() {
        // `>2mm` => Range { lower: Some(2mm), upper: None, lower_inclusive: false, upper_inclusive: true }
        let kind = parse_let_expr("structure S { let r = >2mm }");
        match kind {
            ExprKind::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(upper.is_none(), "upper should be None for `>2mm`");
                assert!(!lower_inclusive, "lower should be exclusive for `>`");
                assert!(upper_inclusive, "absent upper_inclusive should be vacuous true");
                let lower_expr = lower.expect("lower should be Some for `>2mm`");
                match lower_expr.kind {
                    ExprKind::QuantityLiteral { value, unit } => {
                        assert!((value - 2.0).abs() < f64::EPSILON);
                        assert_eq!(unit, UnitExpr::Unit("mm".to_string()));
                    }
                    other => panic!("expected QuantityLiteral for bound, got {:?}", other),
                }
            }
            other => panic!("expected ExprKind::Range, got {:?}", other),
        }
    }

    #[test]
    fn single_sided_range_gte_lower_inclusive() {
        // `>=2mm` => Range { lower: Some(2mm), upper: None, lower_inclusive: true, upper_inclusive: true }
        let kind = parse_let_expr("structure S { let r = >=2mm }");
        match kind {
            ExprKind::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(upper.is_none(), "upper should be None for `>=2mm`");
                assert!(lower_inclusive, "lower should be inclusive for `>=`");
                assert!(upper_inclusive, "absent upper_inclusive should be vacuous true");
                let lower_expr = lower.expect("lower should be Some for `>=2mm`");
                match lower_expr.kind {
                    ExprKind::QuantityLiteral { value, unit } => {
                        assert!((value - 2.0).abs() < f64::EPSILON);
                        assert_eq!(unit, UnitExpr::Unit("mm".to_string()));
                    }
                    other => panic!("expected QuantityLiteral for bound, got {:?}", other),
                }
            }
            other => panic!("expected ExprKind::Range, got {:?}", other),
        }
    }

    #[test]
    fn single_sided_range_lt_upper_exclusive() {
        // `<100MPa` => Range { lower: None, upper: Some(100MPa), lower_inclusive: true, upper_inclusive: false }
        let kind = parse_let_expr("structure S { let r = <100MPa }");
        match kind {
            ExprKind::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_none(), "lower should be None for `<100MPa`");
                assert!(lower_inclusive, "absent lower_inclusive should be vacuous true");
                assert!(!upper_inclusive, "upper should be exclusive for `<`");
                let upper_expr = upper.expect("upper should be Some for `<100MPa`");
                match upper_expr.kind {
                    ExprKind::QuantityLiteral { value, unit } => {
                        assert!((value - 100.0).abs() < f64::EPSILON);
                        assert_eq!(unit, UnitExpr::Unit("MPa".to_string()));
                    }
                    other => panic!("expected QuantityLiteral for bound, got {:?}", other),
                }
            }
            other => panic!("expected ExprKind::Range, got {:?}", other),
        }
    }

    #[test]
    fn single_sided_range_lte_upper_inclusive() {
        // `<=100MPa` => Range { lower: None, upper: Some(100MPa), lower_inclusive: true, upper_inclusive: true }
        let kind = parse_let_expr("structure S { let r = <=100MPa }");
        match kind {
            ExprKind::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_none(), "lower should be None for `<=100MPa`");
                assert!(lower_inclusive, "absent lower_inclusive should be vacuous true");
                assert!(upper_inclusive, "upper should be inclusive for `<=`");
                let upper_expr = upper.expect("upper should be Some for `<=100MPa`");
                match upper_expr.kind {
                    ExprKind::QuantityLiteral { value, unit } => {
                        assert!((value - 100.0).abs() < f64::EPSILON);
                        assert_eq!(unit, UnitExpr::Unit("MPa".to_string()));
                    }
                    other => panic!("expected QuantityLiteral for bound, got {:?}", other),
                }
            }
            other => panic!("expected ExprKind::Range, got {:?}", other),
        }
    }

    #[test]
    fn two_sided_range_inclusive_regression() {
        // `2mm..10mm` => Range { lower: Some, upper: Some, lower_inclusive: true, upper_inclusive: true }
        // Guards that the existing two-sided path is not broken by the single-sided branch.
        let kind = parse_let_expr("structure S { let r = 2mm..10mm }");
        match kind {
            ExprKind::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_some(), "lower should be Some for two-sided range");
                assert!(upper.is_some(), "upper should be Some for two-sided range");
                assert!(lower_inclusive, "lower should be inclusive for `..`");
                assert!(upper_inclusive, "upper should be inclusive for `..`");
            }
            other => panic!("expected ExprKind::Range, got {:?}", other),
        }
    }

    #[test]
    fn two_sided_range_exclusive_upper_regression() {
        // `0mm..<10mm` => Range { lower: Some, upper: Some, lower_inclusive: true, upper_inclusive: false }
        // Guards that the `..<` exclusive-upper detection loop is not broken.
        let kind = parse_let_expr("structure S { let r = 0mm..<10mm }");
        match kind {
            ExprKind::Range {
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => {
                assert!(lower.is_some(), "lower should be Some for two-sided range");
                assert!(upper.is_some(), "upper should be Some for two-sided range");
                assert!(lower_inclusive, "lower should be inclusive for `..<`");
                assert!(!upper_inclusive, "upper should be exclusive for `..<`");
            }
            other => panic!("expected ExprKind::Range, got {:?}", other),
        }
    }
}
