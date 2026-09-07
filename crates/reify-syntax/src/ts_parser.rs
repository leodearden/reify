//! Tree-sitter based parser for the Reify language.
//!
//! Parses source text into tree-sitter CST, then lowers to the `ParsedModule` AST.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

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
pub fn parse_with_prelude_enums<'a>(
    source: &'a str,
    module_path: ModulePath,
    prelude_enum_names: &[&'a str],
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
        declared_module_path: lowering.declared_module_path,
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

/// Classification of an out-of-range numeric literal parse result.
///
/// Used by [`Lowering::classify_number_range`] and consumed by
/// [`Lowering::check_number_range`] to emit a diagnostic at the correct
/// lowering site.
enum NumberRangeViolation {
    /// The parsed value is `+Inf` — the literal overflows `f64::MAX`.
    Overflow,
    /// The parsed value is `0.0` but the significand has a nonzero digit —
    /// the literal underflowed below the smallest representable f64 subnormal.
    Underflow,
}

/// CST → AST lowering context.
struct Lowering<'a> {
    source: &'a str,
    declarations: Vec<Declaration>,
    /// Interior mutability so that `&self` expression-lowering methods can emit diagnostics.
    errors: RefCell<Vec<ParseError>>,
    /// Enum names collected in the first pass for disambiguation.
    known_enums: HashSet<&'a str>,
    /// Module-namespace bindings introduced by this file's `import`
    /// declarations, collected in the SAME order-independent first pass as
    /// `known_enums` (task 5495 μ). Only `ImportKind::Aliased` (the alias) and
    /// `ImportKind::Module` (the final path segment) contribute — see
    /// `collect_import_bindings` for why the entity-binding kinds do not.
    /// Read by `lower_namespaced_call`'s qualifier gate.
    namespace_bindings: HashSet<String>,
    /// The names this file's imports bind NON-namespace-wise, each mapped to the
    /// `ImportKind` that bound it (task 5495 μ). Populated by the same
    /// `collect_import_bindings` pass, from exactly the three kinds
    /// `namespace_bindings` deliberately skips — `Entity`, `EntityAliased` and
    /// `Destructured`.
    ///
    /// This map is what lets `lower_namespaced_call` tell "this name is bound,
    /// but as an entity" from "this name is not bound at all". Both are
    /// rejections, but only the latter is fixed by declaring an import: telling
    /// an author who wrote `import a.b.Widget` to "declare one as `import
    /// <path>.Widget`" hands back the line they already wrote.
    entity_bindings: HashMap<String, ImportKind>,
    /// Module-level pragmas collected during source-file lowering.
    module_pragmas: Vec<Pragma>,
    /// Structured module path from a top-of-file `module a.b.c` declaration.
    /// `None` if no module declaration was present in the source file.
    declared_module_path: Option<ModulePath>,
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
    fn with_prelude_enums(source: &'a str, prelude_enum_names: &[&'a str]) -> Self {
        let mut known_enums: HashSet<&'a str> = HashSet::new();
        for &name in prelude_enum_names {
            known_enums.insert(name);
        }
        Self {
            source,
            declarations: Vec::new(),
            errors: RefCell::new(Vec::new()),
            known_enums,
            namespace_bindings: HashSet::new(),
            entity_bindings: HashMap::new(),
            module_pragmas: Vec::new(),
            declared_module_path: None,
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

    /// Check if a node has an anonymous 'priv' keyword child.
    ///
    /// Mirrors `has_aux_keyword`. Used by `lower_param`, `lower_sub`,
    /// `lower_port`, `lower_let`, and `lower_constraint` to set `is_priv`
    /// (PRD §D-3/D-4, task 3976 step-6; task 4755 extends to let/constraint).
    fn has_priv_keyword(&self, node: tree_sitter::Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !child.is_named() && self.node_text(child) == "priv" {
                return true;
            }
        }
        false
    }

    // ── Top-level lowering ──────────────────────────────────

    fn lower_source_file(&mut self, node: tree_sitter::Node) {
        // First pass: collect enum names for disambiguation of member_access
        // vs EnumAccess in expressions, and the module-namespace bindings this
        // file's imports introduce. This enables order-independent declarations.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_declaration"
                && let Some(name_node) = child.child_by_field_name("name")
            {
                self.known_enums.insert(self.node_text(name_node));
            }
            if child.kind() == "import_declaration" {
                self.collect_import_bindings(child);
            }
        }

        // Second pass: lower all declarations.
        // Annotations immediately before a declaration are accumulated in
        // `pending_annotations` and drained into the declaration's `annotations` field.
        // `#cfg(...)` pragmas immediately before an import are accumulated in
        // `pending_cfg` and drained into the import's `cfg_predicates` field.
        let mut pending_annotations: Vec<Annotation> = Vec::new();
        let mut pending_cfg: Vec<Pragma> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "structure_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_structure(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Structure(decl));
                    }
                }
                "occurrence_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_occurrence(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Occurrence(decl));
                    }
                }
                "import_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let cfg_predicates = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_import(child) {
                        decl.annotations = annotations;
                        decl.cfg_predicates = cfg_predicates;
                        self.declarations.push(Declaration::Import(decl));
                    }
                }
                "enum_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_enum(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Enum(decl));
                    }
                }
                "function_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_function(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Function(decl));
                    }
                }
                "trait_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_trait(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Trait(decl));
                    }
                }
                "field_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_field(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Field(decl));
                    }
                }
                "purpose_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_purpose(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Purpose(decl));
                    }
                }
                "constraint_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_constraint_def(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Constraint(decl));
                    }
                }
                "unit_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_unit(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Unit(decl));
                    }
                }
                "type_alias_declaration" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_type_alias(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::TypeAlias(decl));
                    }
                }
                "joint_definition" => {
                    let annotations = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(mut decl) = self.lower_joint(child) {
                        decl.annotations = annotations;
                        self.declarations.push(Declaration::Joint(decl));
                    }
                }
                "default_declaration" => {
                    // Defaults are not annotatable in v1. Emit a diagnostic for each
                    // annotation/cfg that preceded this declaration so it is not
                    // silently dropped — the author can see the annotation was ignored.
                    let dropped_annotations = std::mem::take(&mut pending_annotations);
                    let dropped_cfg = std::mem::take(&mut pending_cfg);
                    for ann in &dropped_annotations {
                        self.push_error(
                            format!(
                                "annotation '@{}' on a default declaration is not supported; \
                                 defaults are not annotatable in v1",
                                ann.name
                            ),
                            ann.span,
                        );
                    }
                    for cfg in &dropped_cfg {
                        self.push_error(
                            format!(
                                "'#[{}]' attribute on a default declaration is not supported; \
                                 defaults are not annotatable in v1",
                                cfg.name
                            ),
                            cfg.span,
                        );
                    }
                    if let Some(decl) = self.lower_default_decl(child) {
                        self.declarations.push(Declaration::Default(decl));
                    }
                }
                "annotation" => {
                    if let Some(annotation) = self.lower_annotation(child) {
                        pending_annotations.push(annotation);
                    }
                }
                "pragma" => {
                    if let Some(pragma) = self.lower_pragma(child) {
                        if pragma.name == "cfg" {
                            pending_cfg.push(pragma.clone());
                        }
                        self.module_pragmas.push(pragma);
                    }
                }
                "module_declaration" => {
                    // Top-of-file `module a.b.c` declaration.
                    // Extract the dotted path by collecting `identifier` children
                    // of the `path` (import_path) field — mirrors lower_import's
                    // segment-collection loop.
                    let _ = std::mem::take(&mut pending_cfg);
                    if let Some(path_node) = child.child_by_field_name("path") {
                        let mut segments = Vec::new();
                        let mut seg_cursor = path_node.walk();
                        for seg in path_node.children(&mut seg_cursor) {
                            if seg.kind() == "identifier" {
                                segments.push(self.node_text(seg).to_string());
                            }
                        }
                        let dotted = segments.join(".");
                        let span = self.span(child);
                        // Only treat this as a valid top-of-file declaration if
                        // no declarations or errors have been accumulated yet.
                        // When tree-sitter error-recovers by wrapping preceding
                        // content in an ERROR node, that ERROR arm runs first and
                        // pushes a parse error, so `errors` is non-empty here —
                        // in that case we emit an error for the misplaced decl
                        // and leave `declared_module_path` as `None`.
                        let is_at_top = self.declarations.is_empty()
                            && self.errors.borrow().is_empty();
                        if is_at_top {
                            let module_decl = ModuleDecl {
                                path: dotted.clone(),
                                span,
                                content_hash: self.content_hash(child),
                            };
                            self.declarations.push(Declaration::Module(module_decl));
                            self.declared_module_path = ModulePath::from_dotted(&dotted).ok();
                        } else {
                            self.push_error(
                                format!(
                                    "module declaration must be at the top of the file: {}",
                                    dotted
                                ),
                                span,
                            );
                        }
                    }
                }
                "ERROR" => {
                    // Consume any pending annotations and pending cfg so they don't
                    // leak past a syntax error to the next successfully-parsed declaration.
                    let _ = std::mem::take(&mut pending_annotations);
                    let _ = std::mem::take(&mut pending_cfg);
                    self.push_error(
                        format!("syntax error: {}", self.node_text(child)),
                        self.span(child),
                    );
                }
                _ => self.warn_unexpected_child(child, "source file"),
            }
        }
    }

    /// Record the module-namespace binding, if any, that one
    /// `import_declaration` introduces (task 5495 μ; PRD
    /// `docs/prds/v0_6/stdlib-namespace.md` D-7).
    ///
    /// D-7 defines the qualifier of a qualified reference as the import's
    /// binding name — the alias if `as` was used, else the final path segment —
    /// so exactly two of the five `ImportKind`s contribute:
    ///
    /// - `Aliased { alias }` → the alias (`import a.b as pp` binds `pp`)
    /// - `Module` → the final `.`-segment of the path (`import a.b` binds `b`)
    ///
    /// `Entity`, `EntityAliased` and `Destructured` deliberately contribute
    /// NOTHING: they bind ENTITY names, not module namespaces. `Widget.mk()`
    /// (from `import a.b.Widget`) is a method call on an entity — syntax Reify
    /// does not have, and a hard parse error before μ — so binding those kinds
    /// would reopen a narrower version of the very hole this gate closes.
    ///
    /// Those three kinds are still recorded, in `entity_bindings`, mapped to the
    /// `ImportKind` that bound them. They must not qualify a call, but they are
    /// not UNBOUND either, and the two cases need different remedies: only the
    /// wholly-unbound one is fixed by declaring an import. Both maps are filled
    /// from the same `lower_import` call, so a name can never be classified one
    /// way here and another way by the import system.
    ///
    /// The name recorded is the one the AUTHOR WRITES, which for `EntityAliased`
    /// is the alias (`import a.b.Widget as W` is used as `W`), not the entity.
    ///
    /// The classification is obtained by CALLING `lower_import` and matching the
    /// returned `ImportKind`, rather than re-deriving Module-vs-Entity from the
    /// CST: the uppercase-final-segment heuristic then has exactly one
    /// implementation and this gate cannot drift from what the import system
    /// actually binds. The second call is pure — `lower_import` pushes no
    /// diagnostic — so no error is duplicated by lowering the node twice.
    ///
    /// Called from `lower_source_file`'s FIRST pass, the same order-independent
    /// pass that seeds `known_enums`, so an `import parts as pp` written after
    /// the structure that uses `pp.Pulley()` still binds.
    ///
    /// **This file's OWN imports only — no external seeding hook (task ν).**
    /// `known_enums` can be pre-seeded from outside the file
    /// (`parse_with_prelude_enums` → `with_prelude_enums`); these two maps
    /// cannot — by construction they see only the `import_declaration` nodes of
    /// the file being parsed. So a qualifier that `std.prelude` supplies, or
    /// that a facade re-exports via `pub import` (PRD D-4, §10 Q1), is absent
    /// from this file's import set and `lower_namespaced_call`'s gate rejects
    /// it at PARSE time.
    ///
    /// The enum precedent is deliberately not mirrored yet: a
    /// `with_prelude_bindings` seed has no caller until the N3 prelude work
    /// (PRD §8 ι) ships `std.prelude`, so it would be untested surface today.
    /// Widening the gate to prelude-supplied and re-exported bindings is
    /// ν's (task 5505), alongside the qualified lookup that gives such a
    /// binding a module to resolve against.
    fn collect_import_bindings(&mut self, node: tree_sitter::Node) {
        let Some(import) = self.lower_import(node) else {
            return;
        };
        let namespace_binding = match &import.kind {
            ImportKind::Aliased { alias } => Some(alias.clone()),
            ImportKind::Module => import.path.rsplit('.').next().map(str::to_string),
            ImportKind::Entity(_) | ImportKind::EntityAliased { .. } => None,
            ImportKind::Destructured(_) => None,
        };
        if let Some(binding) = namespace_binding.filter(|b| !b.is_empty()) {
            self.namespace_bindings.insert(binding);
            return;
        }

        let entity_names: Vec<String> = match &import.kind {
            ImportKind::Entity(entity) => vec![entity.clone()],
            ImportKind::EntityAliased { alias, .. } => vec![alias.clone()],
            ImportKind::Destructured(names) => names.clone(),
            ImportKind::Aliased { .. } | ImportKind::Module => Vec::new(),
        };
        for name in entity_names.into_iter().filter(|n| !n.is_empty()) {
            self.entity_bindings.insert(name, import.kind.clone());
        }
    }

    /// How a diagnostic should describe the NON-namespace binding an import made
    /// for a name, per `ImportKind` (task 5495 μ).
    ///
    /// Kept per-kind rather than a flat "an entity" so the message reflects what
    /// the author actually wrote — an `import a.b.Widget as W` reader needs to
    /// be told `W` is their own alias, not hunt for a `W` in the module.
    ///
    /// Total over all five kinds rather than `unreachable!` on the two
    /// namespace-binding ones: `collect_import_bindings` never records those
    /// here, but a diagnostic path is the last place that should be able to
    /// panic if that ever changes.
    fn entity_binding_note(kind: &ImportKind) -> String {
        match kind {
            // The qualifier IS the entity name here, so naming it again would
            // only repeat the word the message already quoted.
            ImportKind::Entity(_) => "an entity name".to_string(),
            ImportKind::EntityAliased { entity, .. } => {
                format!("an alias for the entity name `{entity}`")
            }
            ImportKind::Destructured(_) => "an entity name destructured from it".to_string(),
            ImportKind::Aliased { .. } | ImportKind::Module => "an entity name".to_string(),
        }
    }

    /// The trailing caveat for the two `ImportKind`s whose entity-ness was
    /// INFERRED rather than written, so the author whose module happens to have
    /// a capitalised path segment has a next step (task 5495 μ).
    ///
    /// `lower_import` classifies a ≥2-segment import by the CAPITALISATION of
    /// its final segment: `import geometry.Shapes` is `Entity("Shapes")` and
    /// `import geometry.Shapes as sh` is `EntityAliased`, so neither binds a
    /// namespace and `Shapes.Circle()` / `sh.Circle()` are rejected above — with
    /// a confident explanation that is simply WRONG if `Shapes` is a module.
    /// Without this sentence the author has no workaround, because their code is
    /// not in fact wrong. The heuristic predates μ; μ is the first feature that
    /// turns it into a user-visible hard error. Resolving module-vs-entity from
    /// the actual module graph instead of from capitalisation is ν's (task 5505)
    /// — see the `entity_bindings` note and PRD D-7.
    ///
    /// `Destructured` gets NO caveat: `import a.b.{C, D}` names its entities
    /// explicitly, so capitalisation played no part and mentioning it would be
    /// noise. The two namespace-binding kinds never reach this path.
    fn entity_binding_capitalisation_hint(kind: &ImportKind) -> String {
        let segment = match kind {
            ImportKind::Entity(entity) => entity,
            ImportKind::EntityAliased { entity, .. } => entity,
            ImportKind::Destructured(_) | ImportKind::Aliased { .. } | ImportKind::Module => {
                return String::new();
            }
        };
        format!(
            ". If `{segment}` names a MODULE rather than an entity, note that the \
             final path segment is classified by capitalisation — a capitalised one \
             is always read as an entity name — so it has to be lowercase to bind a \
             namespace"
        )
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
            cfg_predicates: vec![],
        })
    }

    fn lower_enum(&self, node: tree_sitter::Node) -> Option<EnumDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        // Detect 'pub' keyword by checking anonymous children
        let is_pub = self.has_pub_keyword(node);

        // Emit exactly ONE aggregated diagnostic when the enum_declaration CST
        // contains any ERROR or MISSING node (mirrors the task-3725 type_arg_list
        // idiom).  This robustly covers two distinct tree-sitter error-recovery
        // shapes for malformed variant field declarations:
        //   • Sibling-ERROR shape: `{ field }` (no type annotation) — the variant
        //     collapses to Unit and `{ field }` is hoisted into a sibling ERROR
        //     node that is a direct child of enum_declaration.
        //   • MISSING-inside-variant shape: `{ field: }` (colon, no type) — a
        //     variant_field_decl IS produced, but its 'type' child contains a
        //     MISSING identifier; there is NO sibling ERROR node.
        // Narrowing the fault span to first_error_or_missing_descendant keeps
        // the span tightly focused on the fault rather than the whole enum.
        // Lowering continues after the diagnostic so callers get a partial AST.
        if node.has_error() {
            let fault_span = first_error_or_missing_descendant(node)
                .map(|n| self.span(n))
                .unwrap_or_else(|| self.span(node));
            self.push_error(
                "syntax error in enum declaration".to_string(),
                fault_span,
            );
        }

        // Iterate enum_variant children (grammar production introduced in task α,
        // step-4).  Each enum_variant holds a name field and optionally
        // variant_field_decl children for named-field payloads.
        let mut variants = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "enum_variant"
                && let Some(variant) = self.lower_enum_variant(child)
            {
                variants.push(variant);
            }
        }

        let doc = self.extract_doc_comment(node);

        let type_params = self.lower_type_parameters(node);

        Some(EnumDecl {
            name,
            doc,
            is_pub,
            type_params,
            variants,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    /// Lower a single `enum_variant` CST node to an `EnumVariantDecl`.
    ///
    /// Bare variants (`Point`) produce `VariantPayload::Unit`.
    /// Named-field variants (`Circle { radius: Length }`) produce
    /// `VariantPayload::Named` with fields in source-declaration order.
    fn lower_enum_variant(&self, node: tree_sitter::Node) -> Option<EnumVariantDecl> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();
        let span = self.span(node);

        // Collect variant_field_decl children for named-field payloads.
        let mut fields: Vec<(String, TypeExpr)> = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variant_field_decl" {
                let field_name_node = match child.child_by_field_name("field") {
                    Some(n) => n,
                    // Defensive: tree-sitter error-recovery may produce a
                    // `variant_field_decl` without the expected 'field' child.
                    // Silently elide the affected field rather than panic; a
                    // Named variant whose fields all elide collapses to Unit.
                    // The malformed-declaration diagnostic is emitted by the
                    // enum-level `has_error()` check in `lower_enum` before
                    // this function is called.
                    None => continue,
                };
                let type_node = match child.child_by_field_name("type") {
                    Some(n) => n,
                    // Defensive: same graceful-degradation backstop for a
                    // missing 'type' child; see the 'field' arm above.
                    None => continue,
                };
                let field_name = self.node_text(field_name_node).to_string();
                let type_expr = self.lower_type_expr_node(type_node);
                fields.push((field_name, type_expr));
            }
        }

        let payload = if fields.is_empty() {
            VariantPayload::Unit
        } else {
            VariantPayload::Named(fields)
        };

        Some(EnumVariantDecl { name, payload, span })
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

    /// Dot-join a `namespaced_name` node's `binding` and `name` CST fields.
    ///
    /// Returns `None` if `node` is not a `namespaced_name` or is missing either
    /// field (only reachable on an error-recovery tree).
    ///
    /// **Encoding contract for the resolution phase (task ν).** A qualified
    /// reference rides DOT-JOINED in the existing `String` name slots —
    /// `TypeExprKind::Named { name }`, `SubDecl::structure_name`,
    /// `ExprKind::FunctionCall { name }` — with no new AST variant, because `.`
    /// is not a legal identifier character and `name.contains('.')` is therefore
    /// an unambiguous discriminator for ν's rewrite. Rationale and alternatives:
    /// PRD `docs/prds/v0_6/stdlib-namespace.md` §3.3 NS-Q1/Q3, D-7.
    ///
    /// Joining the two FIELDS — rather than reading the node's source text — is
    /// what normalises interior whitespace, so `pp . Pulley` yields exactly
    /// `"pp.Pulley"` and ν's discriminator never has to cope with a spaced
    /// variant. Pinned by `qualified_type_whitespace_is_normalised` and
    /// `sub_structure_name_whitespace_is_normalised` in
    /// `tests/harness_syntax/namespaced_ref_lowering_tests.rs`.
    ///
    /// **Pre-ν loudness is per-POSITION, not blanket** — measured on this
    /// branch with `target/debug/reify check`:
    ///
    /// - TYPE position is loud on its own: `param p : obj.width` answers
    ///   `error: unresolved type: obj.width` (exit 1).
    /// - `sub` structure_name is loud on its own: `sub s = obj.width()` answers
    ///   `error: sub-component "s" references unknown structure "obj.width"`
    ///   (exit 1).
    /// - EXPRESSION position is NOT, because the compiler has no unknown-function
    ///   diagnostic behind `ExprKind::FunctionCall`. Loudness there is delivered
    ///   by `lower_namespaced_call`'s import-binding guard for an undeclared
    ///   qualifier, and by module resolution (`error: module 'parts' not found`,
    ///   exit 1) for a declared binding whose module is absent.
    ///
    /// That leaves exactly one silent case — declared binding, module resolves,
    /// member does not — which is resolution work and therefore ν's (task 5505).
    /// See `lower_namespaced_call` for the two-guard sequence.
    fn namespaced_name_text(&self, node: tree_sitter::Node) -> Option<String> {
        if node.kind() != "namespaced_name" {
            return None;
        }
        let binding = node.child_by_field_name("binding")?;
        let name = node.child_by_field_name("name")?;
        Some(self.dot_join(binding, name))
    }

    /// The ONE implementation of μ's dot-join, shared by the two rules that
    /// produce a qualified name — `namespaced_name_text` (type + `sub`
    /// position, joining `binding`/`name`) and `lower_namespaced_call`
    /// (expression position, joining the callee's `object`/`member`).
    ///
    /// The two CST rules have different field NAMES, so they cannot share a
    /// single entry point — but the encoding contract the whole of μ rests on
    /// is this join, and it must not drift between the two halves. Extracted
    /// for exactly the reason `lower_call_arguments` gives the argument walk
    /// exactly one implementation.
    ///
    /// Joining the two nodes' texts — rather than reading the parent's source
    /// text — is what normalises interior whitespace, so both `pp . Pulley` and
    /// `pp . Pulley()` yield exactly `"pp.Pulley"`.
    fn dot_join(&self, first: tree_sitter::Node, second: tree_sitter::Node) -> String {
        format!("{}.{}", self.node_text(first), self.node_text(second))
    }

    /// Lower a type_expr node to a TypeExpr. Handles bare identifiers, parameterized types,
    /// qualified associated-type paths (`Beam::Material`, `Beam::(HasMaterial::Material)`),
    /// and namespaced references through an import binding (`pp.Pulley`).
    fn lower_type_expr_node(&self, node: tree_sitter::Node) -> TypeExpr {
        if node.kind() == "type_expr" {
            // type_expr is choice(function_type, parameterized_type, qualified_type,
            // namespaced_name, identifier)
            let child = node.child(0).unwrap_or(node);
            if child.kind() == "function_type" {
                return self.lower_function_type(child);
            }
            if child.kind() == "parameterized_type" {
                return self.lower_parameterized_type(child);
            }
            if child.kind() == "qualified_type" {
                return self.lower_qualified_type(child);
            }
            // Namespaced reference `pp.Pulley` (task 5495 μ) — see
            // `namespaced_name_text` for the dot-joined encoding contract.
            if let Some(dotted) = self.namespaced_name_text(child) {
                return TypeExpr {
                    kind: TypeExprKind::Named {
                        name: dotted,
                        type_args: vec![],
                    },
                    span: self.span(child),
                };
            }
            // bare identifier
            TypeExpr {
                kind: TypeExprKind::Named {
                    name: self.node_text(child).to_string(),
                    type_args: vec![],
                },
                span: self.span(child),
            }
        } else if node.kind() == "function_type" {
            self.lower_function_type(node)
        } else if node.kind() == "parameterized_type" {
            self.lower_parameterized_type(node)
        } else if node.kind() == "qualified_type" {
            self.lower_qualified_type(node)
        } else {
            // NOTE: no un-wrapped `namespaced_name` arm here, deliberately.
            // `namespaced_name` occurs in exactly two grammar positions: as a
            // `type_expr` arm (handled above, through the wrapper) and as a
            // `sub_declaration`'s `structure_name`. The `structure_name` field
            // has exactly two readers — `lower_sub`, which calls
            // `namespaced_name_text` on the node itself, and
            // `lower_match_arm_decl_arm`, which reads it as raw text — so
            // neither routes a bare `namespaced_name` here. An arm for it would
            // be unreachable-and-untested code on a hot lowering path.
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

    /// Bounded error-recovery fallback for a baseless `qualified_type` node.
    ///
    /// Returns a flat `Named` over the whole-node text with empty `type_args`.
    /// This intentionally does NOT call `lower_type_expr_node`: a baseless
    /// `qualified_type` node has `kind == "qualified_type"`, which would dispatch
    /// back to `lower_qualified_type`, find no base again, and recurse without
    /// bound — causing a stack overflow in release builds (where `debug_assert!`
    /// is compiled out).  The bounded whole-node-text placeholder is structurally
    /// wrong but safe; it restores the pre-4601 fallback shape.
    ///
    /// Empirically unreachable from well-formed source: a missing base before `::`
    /// causes tree-sitter to emit an `(ERROR …)` node, and a recoverable-missing
    /// base is inserted as a zero-width `identifier` (so `child_by_field_name("base")`
    /// returns `Some(missing)` and the `Some` arm fires instead).  This helper
    /// exists as a defensive guard aligned with the file's no-stack-overflow norm
    /// (see the iterative `first_error_or_missing_descendant` walk, ts_parser.rs:84-133).
    fn qualified_type_recovery_base(&self, node: tree_sitter::Node) -> TypeExpr {
        TypeExpr {
            kind: TypeExprKind::Named {
                name: self.node_text(node).to_string(),
                type_args: vec![],
            },
            span: self.span(node),
        }
    }

    /// Lower a `qualified_type` CST node to a `TypeExpr`.
    ///
    /// Handles four grammar forms (task 4601 α widened the base to
    /// `choice($.identifier, $.parameterized_type)`):
    /// - Bare:           `Beam::Material`
    ///   → `QualifiedAssoc { base: Named("Beam"), trait_name: None, member: "Material" }`
    /// - Type-param:     `T::Material`
    ///   → `QualifiedAssoc { base: Named("T"),    trait_name: None, member: "Material" }`
    /// - Applied-base:   `Coupling<Prismatic>::MotionValue`
    ///   → `QualifiedAssoc { base: Named("Coupling", [Named("Prismatic")]), trait_name: None, member: "MotionValue" }`
    /// - FORK-G applied: `Coupling<Prismatic>::(HasMotion::MotionValue)`
    ///   → `QualifiedAssoc { base: Named("Coupling", [Named("Prismatic")]), trait_name: Some("HasMotion"), member: "MotionValue" }`
    ///
    /// The base is lowered via `lower_type_expr_node` in the `Some` arm only
    /// (i.e., only when the `base` field is actually present), which dispatches
    /// `parameterized_type → lower_parameterized_type` (carrying `type_args`) and
    /// falls through `identifier → Named { name, type_args: [] }` — so bare/type-param
    /// bases are byte-identical to the pre-4601 output.
    ///
    /// The `None` arm (tree-sitter error-recovery; empirically unreachable from
    /// well-formed source) calls `qualified_type_recovery_base` — a bounded
    /// whole-node-text placeholder — instead of `lower_type_expr_node`, which
    /// would dispatch back to this function and recurse without bound.
    ///
    /// Resolution to a concrete `Type` is deferred to task ιₑ — this function
    /// emits the unresolved AST node only.
    fn lower_qualified_type(&self, node: tree_sitter::Node) -> TypeExpr {
        // `base` field: either a bare identifier (e.g. "Beam", "T") or a
        // parameterized_type (e.g. `Coupling<Prismatic>`).
        //
        // `lower_type_expr_node` is called ONLY in the `Some` arm (base field
        // actually present) — calling it in the `None` arm would re-dispatch
        // this `qualified_type` node and recurse infinitely in release builds.
        let base = match node.child_by_field_name("base") {
            Some(base_node) => Box::new(self.lower_type_expr_node(base_node)),
            None => {
                debug_assert!(
                    false,
                    "lower_qualified_type: missing `base` field in node '{}' at {:?} — \
                     likely tree-sitter error-recovery output; substituting whole-node text",
                    node.kind(),
                    node.range(),
                );
                Box::new(self.qualified_type_recovery_base(node))
            }
        };

        // `trait` field: present only for the disambiguated form `(Trait::Member)`.
        let trait_name = node
            .child_by_field_name("trait")
            .map(|n| self.node_text(n).to_string());

        // `member` field: the associated-type name (present in both forms).
        //
        // Under tree-sitter error recovery this field may be absent; an empty
        // string would be a silent wrong result, so we assert in debug builds.
        let member = match node.child_by_field_name("member") {
            Some(n) => self.node_text(n).to_string(),
            None => {
                debug_assert!(
                    false,
                    "lower_qualified_type: missing `member` field in node '{}' at {:?} — \
                     likely tree-sitter error-recovery output; using empty string",
                    node.kind(),
                    node.range(),
                );
                String::new()
            }
        };

        TypeExpr {
            kind: TypeExprKind::QualifiedAssoc { base, trait_name, member },
            span: self.span(node),
        }
    }

    /// Lower a `function_type` CST node (`(T) -> U`, `(A, B) -> C`, `() -> U`)
    /// to `TypeExprKind::Function` (task 4595).
    ///
    /// The grammar rule is
    ///   `seq('(', commaSep($.type_expr), ')', '->', field('return_type', $.type_expr))`
    /// so the return type is the only named field (`return_type`) and the
    /// parameter types are the positional `type_expr` children preceding it.
    /// We read the return node via `child_by_field_name`, then collect every
    /// other `type_expr` child (distinguished by node id) as a positional
    /// param — mirroring `lower_qualified_type`'s field-driven discipline.
    ///
    /// The `None` (missing return) arm is tree-sitter error-recovery output,
    /// empirically unreachable from well-formed source; it substitutes a
    /// bounded whole-node-text placeholder (same guard as
    /// `lower_qualified_type`'s missing-base arm) rather than recursing.
    fn lower_function_type(&self, node: tree_sitter::Node) -> TypeExpr {
        let return_node = node.child_by_field_name("return_type");
        let return_type = match return_node {
            Some(n) => Box::new(self.lower_type_expr_node(n)),
            None => {
                debug_assert!(
                    false,
                    "lower_function_type: missing `return_type` field in node '{}' at {:?} — \
                     likely tree-sitter error-recovery output; substituting whole-node text",
                    node.kind(),
                    node.range(),
                );
                Box::new(self.qualified_type_recovery_base(node))
            }
        };

        // Positional param types: every direct `type_expr` child that is NOT
        // the `return_type` field node (compared by stable node id).
        let return_id = return_node.map(|n| n.id());
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "type_expr" && Some(child.id()) != return_id {
                params.push(self.lower_type_expr_node(child));
            }
        }

        TypeExpr {
            kind: TypeExprKind::Function {
                params,
                return_type,
            },
            span: self.span(node),
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

    // ── Joint lowering ─────────────────────────────────────────

    /// Lower a `joint_definition` CST node into a `JointDef`.
    ///
    /// Grammar (task α 4395):
    ///   `joint NAME(params) with <dof> = <body>`
    ///
    /// Strategy:
    /// - Reuses `lower_function`'s param-walk, `lower_type_parameters`,
    ///   `has_pub_keyword`, and `extract_doc_comment` for the common prefix.
    /// - `dof`: walks the `dof` (joint_dof) field node and collects every
    ///   `joint_dof_field` child into `JointDofField`. Uniform for single/record:
    ///   the single-form produces a joint_dof wrapping one field; the record-form
    ///   wraps N fields; the lowering collects all children identically.
    /// - `body`: inspects the `body` (joint_body) field node:
    ///   - If it has `relation_member` children → block form → call
    ///     `lower_relation_members` to produce Vec<Expr> (same as RelateDecl).
    ///   - Otherwise → single-expr form → lower the `result` field into a
    ///     1-element Vec<Expr>.
    ///
    /// Scope boundary (α): no DOF self-check, no validate_range, no
    /// Type::Relation enforcement on the body — all deferred to β.
    fn lower_joint(&self, node: tree_sitter::Node) -> Option<JointDef> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let doc = self.extract_doc_comment(node);
        let is_pub = self.has_pub_keyword(node);
        let type_params = self.lower_type_parameters(node);

        // Collect datum params from fn_param_list (mirrors lower_function's
        // param-walk; no `self` receiver in joint params).
        let params = {
            let mut cursor = node.walk();
            let mut params = Vec::new();
            for child in node.children(&mut cursor) {
                if child.kind() == "fn_param_list" {
                    let mut param_cursor = child.walk();
                    for param_child in child.children(&mut param_cursor) {
                        if param_child.kind() == "fn_param"
                            && let Some(p) = self.lower_fn_param(param_child) {
                                params.push(p);
                            }
                    }
                    break;
                }
            }
            params
        };

        // Lower the DOF fields from the `dof` (joint_dof) field node.
        // Both single-form (joint_dof = joint_dof_field) and record-form
        // (joint_dof = '{' joint_dof_field* '}') produce joint_dof_field
        // children; we collect them all uniformly.
        let dof = if let Some(dof_node) = node.child_by_field_name("dof") {
            let mut fields = Vec::new();
            let mut cursor = dof_node.walk();
            for child in dof_node.children(&mut cursor) {
                if child.kind() == "joint_dof_field"
                    && let Some(f) = self.lower_joint_dof_field(child) {
                        fields.push(f);
                    }
            }
            fields
        } else {
            vec![]
        };

        // Lower the body from the `body` (joint_body) field node.
        let body = if let Some(body_node) = node.child_by_field_name("body") {
            // Check if the body node has `relation_member` children (block form).
            let has_relation_members = {
                let mut cursor = body_node.walk();
                body_node.children(&mut cursor).any(|c| c.kind() == "relation_member")
            };
            if has_relation_members {
                // Block form: reuse lower_relation_members (same as RelateDecl).
                self.lower_relation_members(body_node)
            } else if let Some(result_node) = body_node.child_by_field_name("result") {
                // Single-expr form: lower the `result` field into a 1-element Vec.
                self.lower_expr(result_node).map(|e| vec![e]).unwrap_or_default()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Some(JointDef {
            name,
            doc,
            is_pub,
            type_params,
            params,
            dof,
            body,
            span: self.span(node),
            content_hash: self.content_hash(node),
            annotations: vec![],
        })
    }

    /// Lower a `joint_dof_field` CST node into a `JointDofField`.
    ///
    /// Grammar: `field('name', id) ':' field('type', type_expr) optional(seq('in', field('range', _expression)))`
    fn lower_joint_dof_field(&self, node: tree_sitter::Node) -> Option<JointDofField> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let type_node = node.child_by_field_name("type")?;
        let type_expr = self.lower_type_expr_node(type_node);

        let range = node
            .child_by_field_name("range")
            .and_then(|r| self.lower_expr(r));

        Some(JointDofField {
            name,
            type_expr,
            range,
            span: self.span(node),
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
        let (members, pragmas, defaults, structures) = self.lower_purpose_members(node);

        Some(PurposeDef {
            name,
            is_pub,
            type_params,
            params,
            members,
            defaults,
            structures,
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

    /// Lower a `default_declaration` node: `default TypeName = expr`
    ///
    /// Note: unlike `lower_unit`, the `type` field here is a plain `type_expr`
    /// (not a `dimensional_type_expr`), and the `value` is a full `_expression`
    /// (not a binding value). Reads the `type` field via `lower_type_expr_node`
    /// and the `value` field via `lower_expr`. Returns `None` only if either
    /// field is absent (malformed/error-recovery CST).
    fn lower_default_decl(&mut self, node: tree_sitter::Node) -> Option<DefaultDecl> {
        let type_node = node.child_by_field_name("type")?;
        let type_expr = self.lower_type_expr_node(type_node);

        let value_node = node.child_by_field_name("value")?;
        let value = self.lower_expr(value_node)?;

        Some(DefaultDecl {
            type_expr,
            value,
            span: self.span(node),
            content_hash: self.content_hash(node),
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
                let value = Self::strip_underscores_and_parse(text)?;
                let value = self.check_number_range(value, text, self.span(node))?;
                Some(PragmaValue::Number(value))
            }
            "quantity_literal" => {
                let value_node = node.child_by_field_name("value")?;
                let unit_node = node.child_by_field_name("unit")?;
                let value: f64 = Self::strip_underscores_and_parse(self.node_text(value_node))?;
                let value = self.check_number_range(
                    value,
                    self.node_text(value_node),
                    self.span(value_node),
                )?;
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

    fn lower_purpose_members(
        &mut self,
        node: tree_sitter::Node,
    ) -> (Vec<MemberDecl>, Vec<Pragma>, Vec<DefaultDecl>, Vec<StructureDef>) {
        let mut members = Vec::new();
        let mut pragmas = Vec::new();
        let mut defaults = Vec::new();
        let mut structures = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "purpose_member" {
                // purpose_member is a choice node wrapping the actual member, pragma,
                // default_declaration, or (task 4639) structure_definition.
                if let Some(inner) = child.named_child(0) {
                    if inner.kind() == "pragma" {
                        if let Some(pragma) = self.lower_pragma(inner) {
                            pragmas.push(pragma);
                        }
                    } else if inner.kind() == "default_declaration" {
                        if let Some(decl) = self.lower_default_decl(inner) {
                            defaults.push(decl);
                        }
                    } else if inner.kind() == "structure_definition" {
                        if let Some(s) = self.lower_structure(inner) {
                            structures.push(s);
                        }
                    } else if let Some(member) = self.lower_member(inner) {
                        members.push(member);
                    }
                }
            }
        }
        (members, pragmas, defaults, structures)
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
            is_priv: false, // fn-local lets carry no visibility modifier in the grammar
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
        let mut pending_annotations: Vec<Annotation> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "trait_member" {
                // trait_member is a choice node wrapping the actual member, annotation, or pragma.
                if let Some(inner) = child.named_child(0) {
                    if inner.kind() == "annotation" {
                        if let Some(annotation) = self.lower_annotation(inner) {
                            pending_annotations.push(annotation);
                        }
                    } else if inner.kind() == "pragma" {
                        // Annotations before a pragma are consumed/dropped — no defined semantics.
                        let _ = std::mem::take(&mut pending_annotations);
                        if let Some(pragma) = self.lower_pragma(inner) {
                            pragmas.push(pragma);
                        }
                    } else {
                        // Drain pending annotations before lowering the member.
                        let annotations = std::mem::take(&mut pending_annotations);
                        if let Some(mut member) = self.lower_member(inner) {
                            // Attach drained annotations to Fn members only — the only
                            // trait-member kind with a downstream deprecation consumer
                            // (the TraitStaticCall dispatch arm in expr.rs). Other kinds
                            // drain-and-drop: no annotation semantics are defined for them yet.
                            //
                            // Note: the drain-and-attach *pattern* mirrors lower_members
                            // (line ~2145), but the *target kind* is inverted — lower_members
                            // attaches to Param/Let while here we attach to Fn.
                            if let MemberDecl::Fn(ref mut f) = member {
                                f.annotations = annotations;
                            }
                            members.push(member);
                        }
                    }
                }
            } else {
                // Non-trait_member child (e.g. an ERROR recovery node or punctuation token):
                // drain any pending annotations so they cannot leak past a malformed member
                // onto the next valid member. Mirrors the "ERROR" arm in lower_members (~line 2134).
                let _ = std::mem::take(&mut pending_annotations);
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
            "relate_block" => check_and_lower!(
                self,
                child,
                "relate block",
                self.lower_relate_block(child).map(MemberDecl::Relate)
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
            let params = self.lower_auto_params(node);
            Some(Expr {
                kind: ExprKind::Auto { free, params },
                span: self.span(node),
            })
        } else {
            self.lower_expr(node)
        }
    }

    /// Collect the ordered `name = value` params of a parameterized `auto(...)`
    /// CST node (geometric-relations δ, task 4384).
    ///
    /// The grammar (`auto_keyword`, grammar.js:635) has a parameterized arm
    /// `seq($._auto_token, '(', $.auto_param_list, ')')` whose `auto_param_list`
    /// holds `auto_param` children, each `field('name', identifier) '='
    /// field('value', _expression)`. Returns an empty Vec for bare `auto` and
    /// `auto(free)` (neither carries an `auto_param_list` child). δ only
    /// PRESERVES these params in the AST; consuming them is ζ.
    fn lower_auto_params(&self, auto_node: tree_sitter::Node) -> Vec<(String, Expr)> {
        let mut params = Vec::new();
        let mut cursor = auto_node.walk();
        for child in auto_node.children(&mut cursor) {
            if child.kind() != "auto_param_list" {
                continue;
            }
            let mut inner = child.walk();
            for param in child.children(&mut inner) {
                if param.kind() == "auto_param"
                    && let Some(name_node) = param.child_by_field_name("name")
                    && let Some(value_node) = param.child_by_field_name("value")
                    && let Some(value) = self.lower_expr(value_node)
                {
                    params.push((self.node_text(name_node).to_string(), value));
                }
            }
        }
        params
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
            is_priv: self.has_priv_keyword(node),
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
        // Detect 'priv' visibility modifier (PRD §4 D-3, task 4755 step-6).
        let is_priv = self.has_priv_keyword(node);
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
            is_priv,
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
        // Detect 'priv' visibility modifier (PRD §4 D-3, task 4755 step-6).
        let is_priv = self.has_priv_keyword(node);

        Some(ConstraintDecl {
            is_priv,
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
        // A `namespaced_name` structure_name (`sub p = pp.Pulley()`, task 5495 μ)
        // is joined from its `binding`/`name` CST fields so interior whitespace
        // is normalised — see `namespaced_name_text` for the encoding contract
        // that ν's resolution-phase fixup reads.
        let structure_name = self
            .namespaced_name_text(struct_node)
            .unwrap_or_else(|| self.node_text(struct_node).to_string());

        // Detect collection form: `sub name : List<StructName>` by looking for
        // the anonymous `'List'` keyword token among the DIRECT children. An
        // anonymous token's `kind()` IS its literal text, so `kind() == "List"`
        // matches exactly the collection arm's keyword and nothing else.
        //
        // A `self.node_text(child) == "List"` fallback used to sit alongside the
        // kind check; it was removed as part of task α (indexed-sub
        // instantiation) because it matched ANY direct child whose source text
        // happens to be `List` — a `pose` expression that is the bare identifier
        // `List`, and, once α added the indexer clause, a binder named `List`
        // (`sub xs[List in 0..4] = Foo(a: 1)`) or a domain that is the bare
        // identifier `List` (`sub xs[i in List] = Foo(a: 1)`). Any of those
        // silently flipped an *instantiation* into the collection form, which
        // discards `type_args` and skips the `named_argument_list` loop below.
        // The fallback was never load-bearing: the keyword token is always a
        // direct child with kind `"List"`, pinned by
        // `sub_decl_specialization_body_parser_tests::sub_decl_cst_shape_for_list_collection`
        // and by the `List`-named binder/domain cases in
        // `indexed_sub_instantiation_parser_tests`.
        let mut is_collection = false;
        {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "List" {
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
        // γ = task 3806: a specialization_body's param_assignment children are
        // collected into `spec_param_overrides` (PRD §4.2) so an overridden
        // auto-binding resolves identically to a param-default auto. Populated in
        // the specialization_body arm of the match below. `lower_binding_value` is
        // pure, so collecting here alongside the helper's member lowering has no
        // double side effect.
        let mut spec_param_overrides: Vec<(String, Expr)> = Vec::new();
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
                    // This returns only `_member` decls; `param_assignment` nodes
                    // (e.g. `area = 5mm`) are dropped by the helper and collected
                    // separately into `param_overrides` below.
                    let overrides = self.lower_specialization_body_members(overrides_node);
                    // Collect this entry's `param = value` overrides (task 3931 γ),
                    // mirroring the specialization_body arm at ~2555-2565: walk the
                    // overrides body's `param_assignment` children and lower each
                    // `(name, value)` via the shared `lower_binding_value` helper.
                    let mut param_overrides: Vec<(String, Expr)> = Vec::new();
                    let mut po_cursor = overrides_node.walk();
                    for child in overrides_node.children(&mut po_cursor) {
                        if child.kind() == "param_assignment"
                            && let Some(name_node) = child.child_by_field_name("name")
                            && let Some(value_node) = child.child_by_field_name("value")
                        {
                            let param_name = self.node_text(name_node).to_string();
                            if let Some(expr) = self.lower_binding_value(value_node) {
                                param_overrides.push((param_name, expr));
                            }
                        }
                    }
                    let span = self.span(entry);
                    entries.push(KeyedSubMemberEntry { key, overrides, param_overrides, span });
                }
                (None, entries)
            }
            Some(body_node) => {
                // specialization_body: `{ repeat(param_assignment | _member) }`
                // γ = task 3806: collect each param_assignment as (name, value_expr)
                // into `spec_param_overrides` via the shared `lower_binding_value`
                // helper (PRD §4.2). Both auto and non-auto values are captured so
                // the AST is complete; the compiler acts only on ExprKind::Auto
                // entries this task (ε handles non-auto resolution). The helper below
                // independently lowers the `_member` children; `lower_binding_value`
                // is pure so this second walk over param_assignment children has no
                // double side effect.
                let mut value_cursor = body_node.walk();
                for child in body_node.children(&mut value_cursor) {
                    if child.kind() == "param_assignment"
                        && let Some(name_node) = child.child_by_field_name("name")
                        && let Some(value_node) = child.child_by_field_name("value")
                    {
                        let param_name = self.node_text(name_node).to_string();
                        if let Some(expr) = self.lower_binding_value(value_node) {
                            spec_param_overrides.push((param_name, expr));
                        }
                    }
                }
                let members = self.lower_specialization_body_members(body_node);
                (Some(members), Vec::new())
            }
        };

        // Detect 'aux' modifier (PRD §2.2, task 3899 step-6).
        let is_aux = self.has_aux_keyword(node);
        // Lower the optional `at <pose>` clause. The grammar exposes the pose
        // expression as a named field "pose" on the sub_declaration node
        // (grammar.js task 3899 step-2). δ (task 4384) widened the pose field
        // to `choice($._expression, $.auto_keyword)`, making `at` a new auto
        // binding-site; lowering therefore goes through `lower_binding_value`
        // (not `lower_expr`) so `at auto` / `at auto(seed = …)` lower to
        // `ExprKind::Auto { free, params }`. Ordinary pose expressions still
        // fall through to `lower_expr` inside the helper.
        let pose_expr = node
            .child_by_field_name("pose")
            .and_then(|n| self.lower_binding_value(n));

        // Lower the optional inline relate-block from the trailing
        // `at <pose> where { … }` form (geometric-relations δ, task 4384). The
        // grammar attaches it as field "relations" → a `sub_relate_block` node
        // whose `relation_member` children each hold a relation expression.
        // Empty unless the inline `where { }` block is present.
        let relate_relations = node
            .child_by_field_name("relations")
            .map(|n| self.lower_relation_members(n))
            .unwrap_or_default();

        // Lower the optional indexer clause `[<binder> in <domain>]` from the
        // instantiation arm (indexed-sub-instantiation.md §3.1, task α). The
        // grammar exposes the two halves as the named fields "binder" and
        // "domain"; both are absent on the other arms, so both lower to None
        // there. The binder keeps its own narrow span (an unused-binder
        // diagnostic must underline the binder alone).
        //
        // The two halves are lowered JOINTLY, not independently, so the
        // both-`Some`-or-both-`None` pairing invariant documented on
        // `SubDecl::index_binder` is enforced by this code rather than merely
        // asserted: the grammar admits the clause as one indivisible
        // `optional(seq(…))`, but `lower_expr` can still return `None` (an
        // unhandled node kind, or a nested lowering failure), which an
        // independent `.and_then(…)` would turn into a half-populated
        // `Some(binder)` / `None(domain)` pair. β types the domain and may
        // reasonably `expect(...)` the pair; a half-populated `SubDecl` would be
        // a latent panic there. So a failed domain lowering drops BOTH halves
        // and emits a diagnostic instead of silently degrading to a plain sub.
        //
        // Domain lowering uses plain `lower_expr`, NOT `lower_binding_value`:
        // unlike `at <pose>`, the domain is not an `auto` binding site. α stores
        // it syntactically only — checking it is a `Range<Int>` is task β's.
        let (index_binder, index_domain) = match (
            node.child_by_field_name("binder"),
            node.child_by_field_name("domain"),
        ) {
            (Some(binder_node), Some(domain_node)) => match self.lower_expr(domain_node) {
                Some(domain) => {
                    // TODO(#5482): delete this interim rejection when β wires
                    // the count cell.
                    //
                    // The pair is still POPULATED above and below — the AST
                    // contract is what β builds on, so this diagnostic is a
                    // guard, not a lowering failure. Without it α would leave a
                    // silent-miscompile window open: the clause lowers, nothing
                    // reads it, and the declaration elaborates to exactly ONE
                    // instance with the binder resolving against nothing.
                    //
                    // Reported as an ERROR rather than a warning because the
                    // parse layer has no warning channel at all
                    // (`reify_ast::decl::ParseError` is `{message, span}`, no
                    // severity field), and error matches the pre-α baseline
                    // where this very source was a hard parse error. Emitting a
                    // lowering-pass semantic rejection over this channel is the
                    // established shape in this file — cf. `unknown port
                    // direction` and `unsupported forall body kind`.
                    //
                    // Spanned binder-start..domain-end: the two named fields
                    // are the stable, field-derived way to reach the clause
                    // interior. `self.span(node)` would underline the whole
                    // sub, and the `[`/`]` tokens are anonymous.
                    self.push_error(
                        format!(
                            "indexed sub instantiation `sub {name}[{} in …]` is not yet \
                             elaborated (#5482): the indexer clause parses but no compiler \
                             pass reads it, so this declares exactly ONE `{name}` instance \
                             rather than one per index. Remove the indexer clause until \
                             indexed-sub elaboration (#5482) lands.",
                            self.node_text(binder_node),
                        ),
                        SourceSpan::new(
                            binder_node.start_byte() as u32,
                            domain_node.end_byte() as u32,
                        ),
                    );
                    (
                        Some(SpannedIdent {
                            name: self.node_text(binder_node).to_string(),
                            span: self.span(binder_node),
                        }),
                        Some(domain),
                    )
                }
                // Distinct from the rejection above, and deliberately so: this
                // arm reports a MALFORMED domain and drops the pair; that one
                // reports a well-formed but unelaborated clause and KEEPS it.
                //
                // Reachable on a clean CST, not only on ERROR recovery: the
                // domain `a.(b)` is an `instance_qualified_access`, which the
                // grammar admits with ANY `$._expression` inside the parens and
                // `lower_instance_qualified_access` then rejects for the missing
                // `::`, returning None. Pinned by
                // `malformed_indexer_domain_is_reported_and_drops_both_halves`
                // in reify-syntax's harness_syntax::indexed_sub_instantiation
                // parser tests.
                None => {
                    self.push_error(
                        format!(
                            "invalid indexer domain for `sub {name}[{} in …]`: the domain \
                             expression could not be lowered",
                            self.node_text(binder_node),
                        ),
                        self.span(domain_node),
                    );
                    (None, None)
                }
            },
            // A half-present field pair is unreachable through the grammar (the
            // clause is one indivisible `optional(seq(…))`); it can only arise
            // on an ERROR CST node, which already surfaces its own diagnostic.
            // Dropping both halves keeps the pairing invariant total.
            _ => (None, None),
        };

        Some(SubDecl {
            name,
            structure_name,
            type_args,
            args,
            is_collection,
            where_clause,
            body,
            spec_param_overrides,
            keyed_members,
            is_aux,
            is_priv: self.has_priv_keyword(node),
            pose_expr,
            index_binder,
            index_domain,
            relate_relations,
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    /// Lower a `relate_block` CST member (`relate { … }`) into a `RelateDecl`
    /// (geometric-relations δ, task 4384). The body is `repeat(relation_member)`;
    /// an empty `relate { }` lowers to a `RelateDecl` with no relations.
    fn lower_relate_block(&self, node: tree_sitter::Node) -> Option<RelateDecl> {
        Some(RelateDecl {
            relations: self.lower_relation_members(node),
            span: self.span(node),
            content_hash: self.content_hash(node),
        })
    }

    /// Lower the `relation_member` children of a `relate_block` or
    /// `sub_relate_block` CST node into their relation expressions, in source
    /// order (task δ 4384). Each `relation_member` is `field('expr',
    /// $._expression)`; anonymous and non-lowerable children are skipped. Shared
    /// by both relate homes so the member-level and inline forms stay identical.
    fn lower_relation_members(&self, block_node: tree_sitter::Node) -> Vec<Expr> {
        let mut relations = Vec::new();
        let mut cursor = block_node.walk();
        for child in block_node.children(&mut cursor) {
            if child.kind() == "relation_member"
                && let Some(expr_node) = child.child_by_field_name("expr")
                && let Some(expr) = self.lower_expr(expr_node)
            {
                relations.push(expr);
            }
        }
        relations
    }

    /// Lower a `specialization_body` CST node (`{ repeat(param_assignment | _member) }`)
    /// into a `Vec<MemberDecl>`.
    ///
    /// Shared by the `specialization_body` path and the per-entry `overrides` path in
    /// `keyed_member_block` lowering (task 3929) — both block forms parse via the same
    /// `specialization_body` grammar rule and both lower via this helper.
    ///
    /// Dispatch strategy:
    /// - `_member` children → lowered via `lower_member` and returned (single
    ///   source of truth for member lowering).
    /// - `param_assignment` children → collected into `spec_param_overrides` by
    ///   the caller `lower_sub` (task 3806, PRD §4.2).  This helper itself skips
    ///   the param_assignment children and returns only the `_member` MemberDecls.
    ///   Exception: `auto_keyword` values in param_assignments invoke
    ///   `lower_binding_value` here for centralised auto-keyword tracking
    ///   (β = task 3804, PRD §4.2); the binding-value result is otherwise unused
    ///   by this helper.
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
            is_priv: self.has_priv_keyword(node),
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
            "imaginary_literal" => self.lower_imaginary_literal(node),
            "number_literal" => self.lower_number_literal(node),
            "string_literal" => self.lower_string_literal(node),
            "interpolated_string" => self.lower_interpolated_string(node),
            "bool_literal" => self.lower_bool_literal(node),
            "undef_literal" => Some(Expr {
                kind: ExprKind::Undef,
                span: self.span(node),
            }),
            "identifier" => self.lower_identifier(node),
            "function_call" => self.lower_function_call(node),
            "namespaced_call" => self.lower_namespaced_call(node),
            "list_literal" => self.lower_list_literal(node),
            "set_literal" => self.lower_set_literal(node),
            "map_literal" => self.lower_map_literal(node),
            "ad_hoc_selector" => self.lower_ad_hoc_selector(node),
            "index_access" => self.lower_index_access(node),
            "member_access" => self.lower_member_access(node),
            "qualified_access" => self.lower_qualified_access(node),
            "instance_qualified_access" => self.lower_instance_qualified_access(node),
            "trait_method_call" => self.lower_trait_method_call(node),
            "variant_construction" => self.lower_variant_construction(node),
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
                variable_span: self.span(variable_node),
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
                        // Check whether the pattern contains a variant_binding_pattern
                        // (e.g. `Circle { radius: r } => sub x : Foo`).  The broadened
                        // grammar accepts this form at the decl level, but decl-level
                        // named-field binding is out of scope for β — emit a targeted
                        // message rather than the generic lowering-mismatch fallback.
                        let has_named_bind = child
                            .child_by_field_name("pattern")
                            .map(|pattern_node| {
                                let mut c = pattern_node.walk();
                                pattern_node
                                    .children(&mut c)
                                    .any(|ch| ch.kind() == "variant_binding_pattern")
                            })
                            .unwrap_or(false);

                        if has_named_bind {
                            self.push_error(
                                "named-field binding patterns are not supported in \
                                 decl-level match arms"
                                    .to_string(),
                                self.span(child),
                            );
                        } else {
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
            spec_param_overrides: vec![],
            keyed_members: Vec::new(),
            is_aux: false,
            is_priv: false,
            pose_expr: None,
            index_binder: None,
            index_domain: None,
            relate_relations: Vec::new(),
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

    /// Classify whether a parsed numeric value is out of the representable f64 range.
    ///
    /// The `number_literal` grammar (`\d+(\.\d+)?([eE][+-]?\d+)?` plus `0x`/`0b`
    /// radix forms) can never produce the tokens `inf` or `nan`, so any non-finite
    /// result from `f64::parse` is necessarily an overflow past `f64::MAX`.
    ///
    /// For underflow: the exponent only scales the value, so a significand with at
    /// least one nonzero digit that parses to exactly `0.0` must have flushed to
    /// zero below the minimum subnormal.  A genuine zero literal (`0`, `0.0`,
    /// `0e10`) has an all-zero significand and is **not** rejected.  Nonzero
    /// subnormals (`value != 0.0`) are accepted — only total flush-to-zero is
    /// rejected.
    ///
    /// Radix literals (`0x`/`0b`) are excluded from the underflow branch:
    /// they're parsed from integer digits and can only evaluate to `0.0` when
    /// every digit is zero — a genuine zero.  The exclusion is a guard rather
    /// than relying on the global argument about hex zeros, so the invariant is
    /// local and does not depend on [`Self::mantissa_has_nonzero_digit`]'s
    /// decimal-only `e`/`E` split being safe for hex text.
    fn classify_number_range(value: f64, text: &str) -> Option<NumberRangeViolation> {
        if value.is_infinite() {
            return Some(NumberRangeViolation::Overflow);
        }
        // Radix literals cannot underflow: skip the significand scan so that
        // `mantissa_has_nonzero_digit`'s decimal-only e/E split never touches
        // hex digits such as the `E` in `0xE000`.
        let is_radix = text.starts_with("0x")
            || text.starts_with("0X")
            || text.starts_with("0b")
            || text.starts_with("0B");
        if !is_radix && value == 0.0 && Self::mantissa_has_nonzero_digit(text) {
            return Some(NumberRangeViolation::Underflow);
        }
        None
    }

    /// Return `true` if the significand of `text` (the portion before any `e`/`E`)
    /// contains at least one nonzero ASCII digit.
    ///
    /// Used by [`Self::classify_number_range`] to distinguish a genuine zero
    /// literal (`0`, `0.0`, `0e10`) from a nonzero value that underflowed to
    /// `0.0` (e.g. `1e-400`).
    ///
    /// **Decimal-only assumption**: this function splits on `e`/`E` which are
    /// also valid hex digits.  Callers must not pass radix (`0x`/`0b`) text here;
    /// [`Self::classify_number_range`] guards against this before calling.
    fn mantissa_has_nonzero_digit(text: &str) -> bool {
        let significand = text.split(['e', 'E']).next().unwrap_or(text);
        significand.chars().any(|c| c.is_ascii_digit() && c != '0')
    }

    /// Emit a range-violation diagnostic and return `None`, or return `Some(value)`
    /// if the value is in range.
    ///
    /// This is the `&self` counterpart to the pure [`Self::classify_number_range`]
    /// classifier.  All three lowering sites that consume parsed `f64` values
    /// (`lower_number_literal`, `lower_quantity_literal`, `lower_pragma_value`)
    /// call this so the policy cannot diverge.
    fn check_number_range(&self, value: f64, text: &str, span: SourceSpan) -> Option<f64> {
        match Self::classify_number_range(value, text) {
            Some(NumberRangeViolation::Overflow) => {
                self.push_error(
                    format!(
                        "numeric literal `{text}` is out of range: it overflows the maximum \
                         64-bit floating-point value"
                    ),
                    span,
                );
                None
            }
            Some(NumberRangeViolation::Underflow) => {
                self.push_error(
                    format!(
                        "numeric literal `{text}` is out of range: it underflows to zero \
                         (below the smallest representable 64-bit floating-point value)"
                    ),
                    span,
                );
                None
            }
            None => Some(value),
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
        let value =
            self.check_number_range(value, self.node_text(value_node), self.span(value_node))?;
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
    ///   2. **Mul/Div** — `left (*|·|/) right`, left-associative. Dispatch on the
    ///      operator's source TEXT, not node kind: the `op` field aliases the two
    ///      external-scanner tokens (`_unit_mul_op` / `_unit_div_op`), which
    ///      `child_by_field_name` never resolves — which is why the slice is read
    ///      at all. `*` and `·` (U+00B7) are two spellings of one operator and
    ///      both yield [`UnitExpr::Mul`] (task #5784, PRD
    ///      `docs/prds/v0_6/angle-units-surface-convergence.md` §5 C2).
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

        // 2. Mul/Div: `left (*|·|/) right`, left-associative. Two facts drive it:
        //
        //    - The `op` field aliases the external-scanner tokens (`_unit_mul_op`
        //      / `_unit_div_op`), which `child_by_field_name` does NOT expose. So
        //      detect the arm by the `left`+`right` fields and read the operator
        //      from the source slice between them.  Unit ATOMS are contiguous, so
        //      that slice is normally exactly one of `*`, `·` or `/` — but it is
        //      not guaranteed to be: a comment between two parenthesised groups
        //      lands in the slice too (measured: `5(m)/*c*/*(s)` yields
        //      `/*c*/*`), which is why `classify_unit_op` is TOTAL rather than a
        //      three-way match.
        //    - `*` and `·` (U+00B7 MIDDLE DOT, the SI-conventional multiply) are
        //      two spellings of ONE operator, both yielding `UnitExpr::Mul` —
        //      task #5784 / PRD
        //      `docs/prds/v0_6/angle-units-surface-convergence.md` §5 C2.
        //
        //    `classify_unit_op` owns the match; its doc covers why the slice is
        //    matched EXACTLY and why the fallthrough diagnoses rather than
        //    returning a bare `None` (INV-SF-7 `parse-is-value-faithful`).
        if let (Some(left_node), Some(right_node)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) {
            let left = self.lower_unit_expr(left_node)?;
            let right = self.lower_unit_expr(right_node)?;
            let op_start = left_node.end_byte();
            let op_end = right_node.start_byte();
            let op_text = self.source.get(op_start..op_end)?;
            // Classify the RAW slice FIRST, and stop there when it already reads
            // as an operator.  That is the overwhelmingly common case — unit
            // atoms are contiguous, so the slice is normally just `*`, `·` or `/`
            // — and this function runs on every compound quantity literal of
            // every parse, including each keystroke of a GUI reparse.  The
            // comment sweep below costs a `TreeCursor` plus a `Vec` per node, so
            // it must not be paid on that path.
            //
            // Behaviour-preserving, not an approximation: excision can only ever
            // SHRINK the slice, and a slice whose trimmed form is exactly `*`,
            // `·` or `/` cannot contain a comment at all — every comment opens
            // with `/` and is at least two bytes, so its presence would leave the
            // trimmed slice strictly longer than the operator alone.  Hence the
            // raw `Mul`/`Div` answer is the same answer the residue would give.
            //
            // Everything else falls through to the sweep.  Comments are `extras`,
            // so one written between the operands lands INSIDE the slice —
            // measured on a clean parse (`has_error() == false`):
            //
            //   5N/*c*/*m   →   unit_expr(left, block_comment, right)
            //
            // No parens needed, and the `·` spelling behaves identically.  Cut
            // the comment spans out before re-classifying, so a comment-bearing
            // `Mul`/`Div` lowers to `Mul`/`Div`: the lowered tree must agree with
            // what the grammar ACCEPTED, and rejecting a clean parse would trade
            // one INV-SF-7 violation (wrong value) for a spurious error on valid
            // source.  `collect_unit_op_comment_spans` sweeps the SUBTREE rather
            // than this node's direct children — where an `extra` attaches is a
            // property of the generated parser, not of the grammar rule; see its
            // doc for the measured depth case.
            //
            // `op_residue` is bound in THIS scope, not inside the `else`, so the
            // `Unrecognized(&str)` borrow into it outlives the match below.
            let op_residue: Cow<'_, str>;
            let raw_op = classify_unit_op(op_text);
            let op = if matches!(raw_op, UnitOp::Mul | UnitOp::Div) {
                raw_op
            } else {
                let comment_spans = collect_unit_op_comment_spans(node, op_start, op_end);
                op_residue = strip_unit_op_comments(op_text, op_start, &comment_spans);
                classify_unit_op(&op_residue)
            };
            return self.unit_expr_from_classified_op(op, left, right, node);
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

    /// Build the [`UnitExpr`] a classified operator calls for, or drop the
    /// member — loudly for [`UnitOp::Unrecognized`], silently for
    /// [`UnitOp::Missing`].  `node` is the whole `unit_expr` being lowered, and
    /// is what any diagnostic is spanned to.
    ///
    /// Split out of [`Self::lower_unit_expr`] purely as a TEST SEAM, following
    /// the same shape as [`Self::qualified_type_recovery_base`]: no source
    /// reaches the two dropping arms today (see [`UnitOp`]), so a synthetic
    /// classification handed to a real CST node is the only way to observe that
    /// the diagnostic FIRES AT ALL, exactly once, naming the operator verbatim,
    /// and SPANNED to the whole `unit_expr`.  (Its full wording is deliberately
    /// not pinned — see `unit_op_seam_unrecognized_*`.)  Without that seam, the
    /// `push_error` call
    /// below is defensive code whose first execution would be in production —
    /// the shape INV-SF-7 warns about.  `unit_op_seam_*` in this file's `mod
    /// tests` drives all four arms.
    fn unit_expr_from_classified_op(
        &self,
        op: UnitOp<'_>,
        left: UnitExpr,
        right: UnitExpr,
        node: tree_sitter::Node,
    ) -> Option<UnitExpr> {
        match op {
            UnitOp::Mul => Some(UnitExpr::Mul(Box::new(left), Box::new(right))),
            UnitOp::Div => Some(UnitExpr::Div(Box::new(left), Box::new(right))),
            // Silent BY DESIGN, and not the INV-SF-7 shape: error recovery
            // spliced the operands together, so the tree already carries the
            // ERROR/MISSING node `check_and_lower!` reports. The drop is loud
            // where the user observes it — just not reported twice.  A slice
            // that held ONLY comments reduces to this same case, and for the
            // same reason: the operator token is genuinely absent from the
            // source, so the tree is already in error.
            UnitOp::Missing => None,
            // Names the comment-free RESIDUE, which is the operator the user
            // actually wrote; a comment they deliberately put there is not
            // part of the complaint.
            UnitOp::Unrecognized(other) => {
                self.push_error(
                    format!("unrecognized unit operator `{other}` in unit expression"),
                    self.span(node),
                );
                None
            }
        }
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
        let value = self.check_number_range(value, text, self.span(node))?;
        Some(Expr {
            kind: ExprKind::NumberLiteral { value, is_real },
            span: self.span(node),
        })
    }

    /// Desugar an `imaginary_literal` CST node to `complex(0.0, x)`.
    ///
    /// Grammar: `imaginary_literal = seq(field('value', $.number_literal), token.immediate('j'))`.
    /// The `value` child is the mantissa `number_literal`; the `j` suffix is anonymous.
    ///
    /// Desugars to `ExprKind::FunctionCall { name: "complex", args: [re, im] }` where:
    /// - `re` = `NumberLiteral { value: 0.0, is_real: true }` (synthetic zero real part)
    /// - `im` = the lowered mantissa via `lower_number_literal`
    ///
    /// This avoids introducing a new `ExprKind::ImaginaryLiteral` variant (which would
    /// require exhaustive match updates across ~12 files in reify-compiler/eval/lsp).
    fn lower_imaginary_literal(&self, node: tree_sitter::Node) -> Option<Expr> {
        let value_node = node.child_by_field_name("value")?;
        // Lower the mantissa number_literal to get the imaginary-part Expr.
        let im_expr = self.lower_number_literal(value_node)?;
        // Build a synthetic real-part literal: NumberLiteral { value: 0.0, is_real: true }.
        let re_expr = Expr {
            kind: ExprKind::NumberLiteral {
                value: 0.0,
                is_real: true,
            },
            span: self.span(node),
        };
        Some(Expr {
            kind: ExprKind::FunctionCall {
                name: "complex".to_string(),
                args: vec![re_expr, im_expr],
                arg_names: vec![None, None],
            },
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

    /// Lower an `interpolated_string` CST node to `ExprKind::InterpolatedString`.
    ///
    /// Walks the node's named children in source order:
    /// - `string_chunk` → `StringPart::Literal(decode_string_escapes(raw))`.
    /// - `interpolation` → `StringPart::Hole(lower_expr(expr_child))`.
    ///
    /// The opening and closing `"` delimiters are anonymous nodes and are skipped.
    fn lower_interpolated_string(&self, node: tree_sitter::Node) -> Option<Expr> {
        let mut parts = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "string_chunk" => {
                    let raw = self.node_text(child);
                    parts.push(StringPart::Literal(decode_string_escapes(raw)));
                }
                "interpolation" => {
                    // The interpolation node wraps `{ expr }`.  The named child is
                    // the expression (field "expr" in the grammar).
                    //
                    // Robustness: do NOT propagate `?` here.  If the expr field is
                    // absent or lowering fails (MISSING node on malformed input like
                    // `"x {} y"`), emit a diagnostic and *skip* the bad hole — the
                    // surrounding literal chunks are still valid and should survive.
                    // Silently returning `None` for the whole interpolated string
                    // would cause the entire `let` binding to be dropped, which is
                    // a much worse failure mode than a missing-hole diagnostic.
                    let expr_child = match child.child_by_field_name("expr") {
                        Some(n) => n,
                        None => {
                            self.push_error(
                                "interpolated string hole is missing an expression".into(),
                                self.span(child),
                            );
                            continue;
                        }
                    };
                    let expr = match self.lower_expr(expr_child) {
                        Some(e) => e,
                        None => {
                            // `lower_expr` returns `None` for MISSING/unrecognised
                            // nodes (e.g. `(MISSING number_literal)` inserted by
                            // tree-sitter error recovery for an empty hole).
                            // Emit a diagnostic and skip this hole; the string lives.
                            self.push_error(
                                "interpolated string hole contains an invalid expression"
                                    .into(),
                                self.span(child),
                            );
                            continue;
                        }
                    };
                    parts.push(StringPart::Hole(Box::new(expr)));
                }
                // Any other named child (e.g. error-recovery nodes) — skip.
                _ => {}
            }
        }
        Some(Expr {
            kind: ExprKind::InterpolatedString(parts),
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

        let (args, arg_names) = self.lower_call_arguments(node);

        Some(Expr {
            kind: ExprKind::FunctionCall { name, args, arg_names },
            span: self.span(node),
        })
    }

    /// Walk a call node's `argument_list` child and lower every argument,
    /// returning `(args, arg_names)` as two parallel vectors (`arg_names[i]` is
    /// `None` for a positional argument).
    ///
    /// Both call nodes built from `callTail($)` in the grammar share this walk —
    /// `function_call` (`plain(1)`) and `namespaced_call` (`pp.compute(1,
    /// scale: 2)`, task 5495 μ). Having ONE walk is what keeps the qualified and
    /// unqualified paths at exact parity: a change to named/positional handling
    /// cannot land on one and miss the other. Pinned by
    /// `qualified_call_named_and_positional_args_at_parity` in
    /// `tests/harness_syntax/namespaced_ref_lowering_tests.rs`.
    ///
    /// **A REJECTED ARGUMENT KEEPS ITS SLOT.** Dropping it would silently shift
    /// the arity of the enclosing call and re-label every argument after it:
    /// `plain(1, a.b.c(), 3)` measured as a TWO-argument `FunctionCall` before
    /// this was fixed, with `3` sliding into position 1. That matters even
    /// though the enclosing parse always carries an error, because
    /// `reify_compiler`'s `forward_parse_errors` downgrades every parse error to
    /// a WARNING — so a library consumer that compiles and reads diagnostics
    /// sees the mis-arity'd call with no error at all. The slot is filled with
    /// `ExprKind::Undef`, whose documented job is exactly this (it absorbs the
    /// type cascade via `Type::Error`), and any label the argument carried is
    /// preserved so `args`/`arg_names` stay length-matched and aligned.
    ///
    /// The placeholder is pushed ONLY when the failed lowering also pushed a
    /// DIAGNOSTIC. A `None` with no diagnostic is a silent skip — a `line_comment`
    /// or `block_comment` extra sitting inside the parens, or a node kind
    /// `lower_expr` does not dispatch — and those must keep dropping out, or
    /// `f(1, /* c */ 2)` would become a three-argument call on a CLEAN parse.
    /// Measuring the diagnostic count around each argument is what separates the
    /// two cases without a second "is this an argument slot" predicate that could
    /// drift from the grammar (task 5495 μ, amendment; review suggestion #7).
    fn lower_call_arguments(&self, node: tree_sitter::Node) -> (Vec<Expr>, Vec<Option<String>>) {
        let mut args = Vec::new();
        let mut arg_names = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument_list" {
                let mut arg_cursor = child.walk();
                for arg_child in child.children(&mut arg_cursor) {
                    let errors_before = self.errors.borrow().len();
                    if let Some((arg_name, expr)) = self.lower_call_argument(arg_child) {
                        arg_names.push(arg_name);
                        args.push(expr);
                    } else if self.errors.borrow().len() > errors_before {
                        arg_names.push(self.call_argument_label(arg_child));
                        args.push(Expr {
                            kind: ExprKind::Undef,
                            span: self.span(arg_child),
                        });
                    }
                }
            }
        }
        (args, arg_names)
    }

    /// The label of an `argument_list` child that FAILED to lower, so a
    /// placeholder can keep both parallel vectors aligned (task 5495 μ).
    ///
    /// `Some(name)` for a `named_argument` whose `value` was rejected — the
    /// label itself parsed fine and dropping it would turn a named argument into
    /// a positional one — and `None` for everything else.
    fn call_argument_label(&self, node: tree_sitter::Node) -> Option<String> {
        if node.kind() != "named_argument" {
            return None;
        }
        node.child_by_field_name("name")
            .map(|name| self.node_text(name).to_string())
    }

    /// Lower a `namespaced_call` — a call through an import binding,
    /// `pp.Pulley()` / `pp.compute(1, scale: 2)` (task 5495 μ; PRD
    /// `docs/prds/v0_6/stdlib-namespace.md` §3.3 NS-Q2, D-7).
    ///
    /// Emits the ORDINARY `ExprKind::FunctionCall` — the same variant the
    /// unqualified path uses — with the qualifier carried DOT-JOINED in the
    /// existing `name` slot and the arguments produced by the shared
    /// `lower_call_arguments` walk. No new `ExprKind` variant: see
    /// `namespaced_name_text` for the full encoding contract handed to the
    /// resolution phase (task ν), of which this is the expression-position half.
    ///
    /// The qualifier is joined from the callee's `object`/`member` CST FIELDS
    /// via the shared `dot_join`, so `pp . Pulley()` normalises to exactly
    /// `"pp.Pulley"` — the same single implementation `namespaced_name_text`
    /// uses for the type-position form.
    ///
    /// **Two guards, in this order.** Guard 1 checks the callee's SHAPE; guard 2
    /// checks that its qualifier is a DECLARED import binding. The order is
    /// load-bearing: a mis-shaped callee (`a.b.c()`, `arr[0].g()`) has no single
    /// qualifier to name, so it must reach guard 1 first and keep that guard's
    /// own wording.
    ///
    /// **Callee-shape guard (D-7 / PRD §9).** `grammar.js`'s `namespaced_call`
    /// rule takes a full `member_access` as its callee (an inline `identifier
    /// '.' identifier` would collide with `member_access` as a reduce-reduce
    /// ambiguity), and the `member_access` rule's `object` field is in turn a
    /// full `_expression`. So the callee's object is not necessarily a binding
    /// identifier: a 3+-segment path (`a.b.c()`, object = `member_access`)
    /// reaches here, and so does any other postfix chain (`arr[0].g()`, object =
    /// `index_access`; `f(1).g()`, object = `function_call`). Every one of those
    /// is rejected at lowering, worded around what is actually checked — the
    /// callee must be a simple `binding.Name` — with the out-of-scope sentence
    /// appended ONLY for the dotted-path case it describes. Rejection is
    /// unchanged in kind: every one of these forms was an error before μ and
    /// still is, now with a message instead of an anonymous ERROR node. The
    /// rejection lowers nothing, so no fabricated multi-segment name reaches the
    /// AST — and `lower_binding_value` propagates the `None`, so the enclosing
    /// member is dropped rather than half-built.
    ///
    /// **Import-binding guard (D-7).** `namespaced_call` captures EVERY
    /// two-segment `ident.ident(args)`, not only the import-qualified ones.
    /// Without this guard μ turns hard parse errors into SILENCE: `obj.width()`,
    /// `self.w()` and `totally.undefined_thing(1, 2)` were all `Parse error …
    /// exit 1` before μ, and measured as exit 0 after it, because the compiler
    /// has no unknown-function diagnostic behind `ExprKind::FunctionCall`.
    /// Lowering is the first layer that knows the import set
    /// (`namespace_bindings`, seeded by `collect_import_bindings` in
    /// `lower_source_file`'s order-independent first pass), so the gate lives
    /// here rather than on any of the three grammar surfaces — none of which
    /// knows the imports, and restricting the callee inline would reintroduce the
    /// reduce-reduce ambiguity with `member_access` described above.
    ///
    /// It runs strictly AFTER the callee-shape guard so that guard's diagnostics
    /// are untouched, and shares its rejection shape: one `push_error` spanning
    /// the callee, then `return None`.
    ///
    /// After both guards, exactly one silence remains in expression position: a
    /// DECLARED binding whose module resolves but whose member does not. That is
    /// resolution work by definition and therefore ν's (task 5505) — pinned as an
    /// executable case by `declared_binding_with_unknown_member_is_left_to_resolution`
    /// rather than assumed closed. A declared binding whose module is ABSENT is
    /// already loud downstream (`error: module 'parts' not found`, exit 1).
    ///
    /// The call-LESS forms (`pp.Pulley`, `pp.FitClass.Clearance`) are NOT routed
    /// here: they stay `member_access`, indistinguishable from `self.width` at
    /// parse time, and their disambiguation is deferred to ν exactly as
    /// resolution-unification D-9 defers `MemberAccess`→`EnumAccess`.
    fn lower_namespaced_call(&self, node: tree_sitter::Node) -> Option<Expr> {
        let callee = node.child_by_field_name("callee")?;
        let object = callee.child_by_field_name("object")?;
        let member = callee.child_by_field_name("member")?;

        if object.kind() != "identifier" {
            let scope_note = if object.kind() == "member_access" {
                "; bare full-path qualification is out of scope \
                 (docs/prds/v0_6/stdlib-namespace.md §9)"
            } else {
                ""
            };
            self.push_error(
                format!(
                    "unsupported qualified call `{callee_text}(...)`: the callee of a \
                     qualified call must be a simple `binding.Name(...)` through an \
                     `import ... as binding` alias, but `{object_text}` is not a binding \
                     name{scope_note}",
                    callee_text = self.node_text(callee),
                    object_text = self.node_text(object),
                ),
                self.span(callee),
            );
            return None;
        }

        let qualifier = self.node_text(object);
        if !self.namespace_bindings.contains(qualifier) {
            let callee_text = self.node_text(callee);
            let message = match self.entity_bindings.get(qualifier) {
                // Bound — but as an ENTITY name, so "declare an import" is not
                // the remedy: one IS declared. Handing `import a.b.Widget` back
                // the advice "declare one as `import <path>.Widget`" is worse
                // than silence, because it is the line already in the file.
                Some(kind) => format!(
                    "qualifier `{qualifier}` in `{callee_text}(...)` is not a module \
                     namespace: an import in this file binds `{qualifier}`, but as \
                     {binding_note}, and the qualifier of a qualified call must be a \
                     module namespace. Reify has no method-call syntax, so this cannot \
                     be a call on the entity `{qualifier}`{capitalisation_hint}",
                    binding_note = Self::entity_binding_note(kind),
                    capitalisation_hint = Self::entity_binding_capitalisation_hint(kind),
                ),
                // Not bound at all — today's message, verbatim.
                None => format!(
                    "unknown qualifier `{qualifier}` in `{callee_text}(...)`: the qualifier \
                     of a qualified call must be a module namespace bound by an import, but \
                     no import in this file binds `{qualifier}` — declare one as \
                     `import <path> as {qualifier}` or `import <path>.{qualifier}`. Reify has \
                     no method-call syntax, so this cannot be a call on a value named \
                     `{qualifier}`"
                ),
            };
            self.push_error(message, self.span(callee));
            return None;
        }

        let name = self.dot_join(object, member);
        let (args, arg_names) = self.lower_call_arguments(node);

        Some(Expr {
            kind: ExprKind::FunctionCall { name, args, arg_names },
            span: self.span(node),
        })
    }

    /// Lower a single child of `argument_list`, which may be either a bare
    /// `_expression` or a `named_argument`. Returns `(label, value)` where
    /// `label` is `None` for positional arguments and `Some(name)` for named
    /// arguments like `foo(a: 1.0)`.
    ///
    /// The `named_argument` branch delegates to `lower_binding_value` (not
    /// `lower_expr`), making this the **second AST-observable caller** of grammar
    /// slot 5 (`named_argument.value`). The first caller is `lower_named_arg`
    /// (via `named_argument_list` for `sub` instantiations). See
    /// `lower_binding_value`'s doc-comment for the full two-caller enumeration.
    fn lower_call_argument(&self, node: tree_sitter::Node) -> Option<(Option<String>, Expr)> {
        if !node.is_named() {
            return None;
        }
        if node.kind() == "named_argument" {
            let name_node = node.child_by_field_name("name")?;
            let arg_name = self.node_text(name_node).to_string();
            let value_node = node.child_by_field_name("value")?;
            let expr = self.lower_binding_value(value_node)?;
            return Some((Some(arg_name), expr));
        }
        Some((None, self.lower_expr(node)?))
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

        // The shared `callTail($)` argument walk — same helper `function_call`,
        // `namespaced_call` and `trait_method_call` use, so all four call
        // surfaces inherit its slot-preservation invariant: a rejected argument
        // leaves an `Undef` placeholder rather than shifting this call's arity.
        // Ad-hoc selectors don't bind named arguments, so the labels are
        // discarded (unchanged behaviour); only `args`' arity and indexing.
        let (args, _) = self.lower_call_arguments(node);

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

        // Collect positional args through the SHARED `callTail($)` walk
        // `lower_call_arguments` — the one implementation `function_call`,
        // `namespaced_call` and `ad_hoc_selector` also use, so no call surface
        // can drift from the others. The invariant this inherits: a rejected
        // argument leaves an `ExprKind::Undef` placeholder in its own slot
        // rather than being dropped, because dropping it would shift this
        // call's arity and slide every later argument down a position (task
        // 5495 μ, amendment).
        // Trait method calls don't use named-arg binding, so any named-arg label is
        // silently dropped — only the value expression is retained.  Named-arg syntax
        // is grammatically permitted at call sites (e.g. `Trait::method(x: value)`),
        // so dropping the label here is correct and expected.
        let (args, _) = self.lower_call_arguments(node);

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

    /// Lower a `variant_construction` CST node to `ExprKind::VariantConstruct`.
    ///
    /// Grammar (task α, step-6):
    ///   `Name { field: value, ... }` — ≥1 named field, optional trailing comma.
    ///
    /// The lowered node carries the variant name and a Vec of (field_name, Expr)
    /// in source-declaration order.  No `known_enums` gating — whether `Name` is
    /// a real enum variant, and whether the supplied fields match the variant's
    /// declared payload, is resolved by the `variant_construct` compiler checker
    /// (task δ #3942): it emits VariantMissingField / VariantUnknownField /
    /// VariantPayloadType and assembles the literal `Value::Enum` payload.
    fn lower_variant_construction(&self, node: tree_sitter::Node) -> Option<Expr> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.node_text(name_node).to_string();

        let mut fields: Vec<(String, Expr)> = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variant_construction_field" {
                let field_name_node = match child.child_by_field_name("field") {
                    Some(n) => n,
                    // Error-recovery node missing 'field' child — elide it. An
                    // elided construction field surfaces downstream as a δ (#3942)
                    // VariantMissingField / VariantUnknownField diagnostic from the
                    // `variant_construct` field-set checker, so no panic is needed.
                    None => continue,
                };
                let value_node = match child.child_by_field_name("value") {
                    Some(n) => n,
                    // Error-recovery node missing 'value' child — elide (same δ
                    // (#3942) downstream field-set signal as the missing-'field' arm).
                    None => continue,
                };
                let field_name = self.node_text(field_name_node).to_string();
                let value_expr = match self.lower_expr(value_node) {
                    Some(e) => e,
                    // lower_expr returned None for the field value (unsupported or
                    // error-recovery expression kind) — elide rather than panic; the
                    // dropped field surfaces as a δ (#3942) field-set diagnostic.
                    None => continue,
                };
                fields.push((field_name, value_expr));
            }
        }

        Some(Expr {
            kind: ExprKind::VariantConstruct { name, fields },
            span: self.span(node),
        })
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

/// Classification of the operator slice between a `unit_expr`'s `left` and
/// `right` operands — see [`Lowering::lower_unit_expr`], the sole caller.
///
/// Split out of the method so the non-happy-path arms are REACHABLE FROM A TEST:
/// defensive code whose first observation is in production is exactly the shape
/// INV-SF-7 warns about.
///
/// [`UnitOp::Unrecognized`] and [`UnitOp::Missing`] are both DEFENSIVE arms: no
/// probed source reaches either.  They are not therefore removable, and the
/// history says why the fallthrough must DIAGNOSE rather than return a bare
/// `None`.
///
/// Comments are `extras`, so one written between the operands is part of the raw
/// operator slice: `5N/*c*/*m` parses with no ERROR node and yields `/*c*/*`
/// (measured, task #5784).  Three successive contracts for that input —
///   1. before #5784, `op_text.contains('/')` lowered it to `Div`: a well-typed
///      WRONG value from a clean parse, the INV-SF-7 shape itself;
///   2. #5784's exact match made it `Unrecognized("/*c*/*")` — loud, but a
///      spurious error on source the grammar ACCEPTED;
///   3. today, [`strip_unit_op_comments`] cuts the comment spans out first, so
///      the residue `*` classifies as the `Mul` the CST plainly shows.
///
/// READ THAT AS A SEPARATE DEFECT FROM THE `·` WIDENING.  Contract (1) is
/// PRE-EXISTING: `contains('/')` mis-read `/*c*/*` as `Div` for the whole life of
/// `lower_unit_expr`, with or without U+00B7, so nothing about it is `·`-specific.
/// #5784 is what made it observable — the exact match turned a silent wrong value
/// into a loud spurious error — and so repaired it here rather than leaving a
/// known INV-SF-7 wrong-value bug behind a task boundary.  The `·` change itself
/// is two lines: one scanner guard, and the `"·"` arm of [`classify_unit_op`].
/// Everything else on this path — this enum, [`strip_unit_op_comments`],
/// [`collect_unit_op_comment_spans`] and
/// [`Lowering::unit_expr_from_classified_op`] — is the comment-correctness fix.
///
/// After (3) the residue is always exactly the operator token, because the only
/// `extras` are whitespace (trimmed), comments (excised), and a sentinel the
/// scanner never emits.  So `Unrecognized` now guards against a FUTURE operator
/// token reaching this function unhandled: without the diagnostic, a bare `None`
/// out of `lower_unit_expr` propagates through `lower_quantity_literal` and
/// `lower_let` as a DROPPED member with no error at all.
///
/// Both arms are therefore observed ONLY by tests, in two layers — do not delete
/// either as "dead":
///   - `classify_unit_op_*` and `strip_unit_op_comments_*` pin the
///     CLASSIFICATION, i.e. which arm a given slice lands in;
///   - `unit_op_seam_*` pin what the CALL SITE then does with it — that
///     `Unrecognized` emits exactly one error naming the operator verbatim and
///     spanned to the whole `unit_expr`, and that `Missing` emits none.  They
///     drive [`Lowering::unit_expr_from_classified_op`] directly, which exists
///     as that seam.
#[derive(Debug, PartialEq, Eq)]
enum UnitOp<'a> {
    /// `*` or `·` (U+00B7 MIDDLE DOT) — two spellings of ONE operator, both
    /// yielding [`UnitExpr::Mul`] (task #5784).
    Mul,
    /// `/` → [`UnitExpr::Div`].
    Div,
    /// The slice is empty or whitespace-only, i.e. the operator token is
    /// MISSING — spliced away by tree-sitter's error recovery.  The caller must
    /// NOT diagnose: the tree already carries the ERROR/MISSING node that
    /// `check_and_lower!` reports.
    Missing,
    /// Anything else, carried verbatim (already trimmed) so the caller can name
    /// it in the diagnostic.  Never dropped silently.
    Unrecognized(&'a str),
}

/// Classify the operator slice between a `unit_expr`'s two operands.
///
/// The caller classifies the RAW slice first and, only if that does not already
/// read as `Mul`/`Div`, re-classifies the residue left by
/// [`strip_unit_op_comments`].  Either way the trimmed input it finally acts on
/// is exactly `*`, `·` or `/` for every parse the grammar accepts — unit atoms
/// are contiguous and the only other `extras` are whitespace and a never-emitted
/// sentinel.  The match is nonetheless TOTAL; see [`UnitOp`] for why the leftover
/// arms stay.
///
/// Matched EXACTLY rather than by `contains`, and every arm is total, because the
/// caller cannot afford an unhandled operator: a bare `None` out of
/// `lower_unit_expr` propagates through `lower_quantity_literal` and `lower_let`
/// as a DROPPED member, while `check_and_lower!` stays silent (it keys off a CST
/// that `is_error()`, and a scanner-accepted operator produces no error node).
/// The user then sees a binding vanish with no diagnostic — the INV-SF-7
/// `parse-is-value-faithful` failure shape (`docs/legibility/design-invariants.md`).
/// Returning [`UnitOp::Unrecognized`] instead of `None` closes that for the
/// OPERATOR-CLASSIFICATION path — every operator spelling, present or future,
/// rather than `·` alone.
///
/// Scope of that claim, stated exactly because the next reader will lean on it:
/// it covers this function's fallthrough, NOT the whole `Mul`/`Div` arm.  Three
/// bare-`None` exits still precede the classification in
/// [`Lowering::lower_unit_expr`] — the two `self.lower_unit_expr(..)?` operand
/// recursions and the `self.source.get(op_start..op_end)?` slice read.  All
/// three are unreachable for a well-formed CST (the operand byte ranges are
/// token-aligned, ascending and inside `self.source`), so they are not live
/// INV-SF-7 defects; but they are silent, so "no silent exit anywhere in the
/// arm" would be a false claim.  A future edit that can make any of them fail on
/// real source must give it a diagnostic, not inherit this one.
fn classify_unit_op(op_text: &str) -> UnitOp<'_> {
    match op_text.trim() {
        "*" | "·" => UnitOp::Mul,
        "/" => UnitOp::Div,
        "" => UnitOp::Missing,
        other => UnitOp::Unrecognized(other),
    }
}

/// Collect the byte ranges of every comment lying inside a `unit_expr`'s raw
/// operator slice `[op_start, op_end)` — the `cuts` argument
/// [`strip_unit_op_comments`] expects, ascending and non-overlapping.
///
/// Sweeps the SUBTREE, not just `node`'s direct children, because WHERE
/// tree-sitter attaches an `extra` is a property of the GENERATED parser, not of
/// the grammar rule: it can move under a `grammar.js` edit that changes no
/// accepted language (narrowing the paren arm to a hidden `_unit_atom`, which
/// `grammar.js`'s own TODO contemplates, is the concrete candidate).  Comments
/// already DO attach at depth — measured on a clean parse (task #5784 amendment
/// pass):
///
/// ```text
///   5(m/*c*/)*s  →  unit_expr(unit_expr(unit_expr(unit_name), block_comment),
///                             unit_expr(unit_name))
/// ```
///
/// i.e. a child of the INNER `unit_expr`, not the outer one.  That comment is
/// outside the operator slice (`*`) and so is filtered out, but it shows the
/// attachment point is not fixed at direct-child.  Were an IN-SLICE comment ever
/// to move that way, a direct-children sweep would miss it, the residue would
/// still carry the comment text, and [`Lowering::lower_unit_expr`] would emit a
/// spurious `unrecognized unit operator` on source the grammar ACCEPTED — the
/// exact failure the excision path exists to remove, and one no test would
/// localise to attachment.  Widening costs nothing on the hot path: the caller
/// only gets here when the RAW slice failed to classify.
///
/// An ANCESTOR attachment needs no handling: sibling ranges are disjoint and
/// ordered, so a node lying strictly inside `node`'s range cannot be a child of
/// any ancestor of `node`.  A descendant walk is complete for `[op_start,
/// op_end)`.
///
/// Pre-order with children in source order, and a matched comment is never
/// descended into (comments do not nest), so the spans come back ascending and
/// non-overlapping.  The range filter is what keeps the wider walk safe, and
/// [`strip_unit_op_comments`] re-validates every range regardless.
fn collect_unit_op_comment_spans(
    node: tree_sitter::Node,
    op_start: usize,
    op_end: usize,
) -> Vec<(usize, usize)> {
    fn walk(
        node: tree_sitter::Node,
        op_start: usize,
        op_end: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let (start, end) = (child.start_byte(), child.end_byte());
            // Siblings are ordered and non-overlapping, so a child disjoint from
            // the operator slice cannot contain an in-slice comment either.
            if end <= op_start || start >= op_end {
                continue;
            }
            if matches!(child.kind(), "line_comment" | "block_comment") {
                if start >= op_start && end <= op_end {
                    out.push((start, end));
                }
                continue;
            }
            walk(child, op_start, op_end, out);
        }
    }
    let mut spans = Vec::new();
    walk(node, op_start, op_end, &mut spans);
    spans
}

/// Cut the comment spans out of a `unit_expr`'s raw operator slice, returning
/// what [`classify_unit_op`] should see.
///
/// Comments are parser `extras`, so one written between the operands sits INSIDE
/// the slice `lower_unit_expr` cuts from the source — as a descendant of the
/// `unit_expr` node (a DIRECT child in every shape probed so far, but
/// [`collect_unit_op_comment_spans`] does not assume that), on a tree with no
/// ERROR node anywhere.  Measured (task #5784 amendment pass):
///
/// ```text
///   5N/*c*/*m        →  unit_expr(left, block_comment, right)   slice `/*c*/*`
///   5N/*c*/·m        →  same shape                              slice `/*c*/·`
///   5N/*c*//m        →  same shape                              slice `/*c*//`
///   5N/*a*//*b*/*m   →  two block_comment children              slice `/*a*//*b*/*`
/// ```
///
/// `line_comment` is accepted by the caller's filter for symmetry, but was never
/// observed inside a `unit_expr`: a `//…` comment ends the line, and every probed
/// spelling (`5N//c⏎*m`, `5(m)//c⏎*(s)`) reparsed as a `binary_expression`
/// instead.
///
/// Classifying those raw slices would reject source the GRAMMAR ACCEPTED, so the
/// comments come out first and the residue (`*`, `·`, `/`) classifies as the
/// operator the CST plainly shows.
///
/// `slice_start` is `slice`'s byte offset in the source file; `cuts` are ABSOLUTE
/// `(start, end)` byte ranges which the caller has already filtered to those
/// lying inside the slice, in ascending source order.  Any entry that does not
/// translate to an ascending, in-bounds, char-boundary-aligned range inside
/// `slice` is SKIPPED, and a slice whose tail cannot be taken falls back to the
/// raw text: a surprising CST then degrades to the loud `Unrecognized`
/// diagnostic rather than to a panic or a silently wrong operator.
///
/// Borrows rather than allocating when `cuts` is empty.  The caller goes further
/// and does not call this at all unless the raw slice failed to classify, so a
/// comment-free unit expression pays neither this nor the `TreeCursor` + `Vec`
/// needed to find the cuts.
fn strip_unit_op_comments<'a>(
    slice: &'a str,
    slice_start: usize,
    cuts: &[(usize, usize)],
) -> Cow<'a, str> {
    if cuts.is_empty() {
        return Cow::Borrowed(slice);
    }
    let mut out = String::with_capacity(slice.len());
    // Slice-relative offset of the first byte not yet copied into `out`.
    let mut kept_to = 0usize;
    for &(start, end) in cuts {
        let (Some(rel_start), Some(rel_end)) = (
            start.checked_sub(slice_start),
            end.checked_sub(slice_start),
        ) else {
            continue;
        };
        if rel_start < kept_to || rel_end < rel_start || rel_end > slice.len() {
            continue;
        }
        let Some(keep) = slice.get(kept_to..rel_start) else {
            continue;
        };
        out.push_str(keep);
        kept_to = rel_end;
    }
    match slice.get(kept_to..) {
        Some(tail) => {
            out.push_str(tail);
            Cow::Owned(out)
        }
        // `kept_to` landed off a char boundary — unreachable for a token-aligned
        // CST.  Hand back the raw slice so the caller diagnoses loudly.
        None => Cow::Borrowed(slice),
    }
}

/// Decode escape sequences in a raw `string_chunk` token from an interpolated string.
///
/// Translations applied:
/// - `\n` → newline
/// - `\t` → tab
/// - `\\` → backslash
/// - `\"` → double-quote
/// - `{{` → `{`  (doubled brace is content, not an interpolation start)
/// - `}}` → `}`
/// - `\X` for any other X → `X`  (lenient: drop the backslash, keep the char)
///
/// This is the shared unescape helper anticipated by the comment at ts_parser.rs:2174.
/// Only brace-bearing strings reach this function; plain `string_literal` nodes
/// are left raw by `lower_string_literal` (fast path, no braces, no decoding).
fn decode_string_escapes(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => out.push(other),
                None => {}
            },
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                } else {
                    out.push('{');
                }
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    out.push('}');
                } else {
                    out.push('}');
                }
            }
            other => out.push(other),
        }
    }
    out
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
                MemberDecl::Relate(_) => "relate".into(),
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
            ExprKind::FunctionCall { name, args, .. } => {
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
                MemberDecl::Relate(r) => r.span,
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
                MemberDecl::Relate(_) => {}
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
            assert_eq!(ty_text, "Length", "width type text");
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
                MemberDecl::Relate(r) => (r.span, r.content_hash),
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
    param width: Length = 80mm
    param !!!invalid!!!
    param height: Length = 100mm
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
                MemberDecl::Relate(r) => (r.content_hash, r.span),
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
                MemberDecl::Relate(r) => (r.content_hash, r.span),
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
    param volume: Length = 100mm
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
    param thickness: Length = 5mm
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
    param width: Length = 80mm
    param height: Length = 100mm
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
    param w: Length = 80mm
    param h: Length = 100mm
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
    param x: Length = 5mm
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
        let source = "enum Direction { In, Out, Bidi }\nstructure S { param x: Length = 5mm }";
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
                let variant_names: Vec<&str> =
                    e.variants.iter().map(|v| v.name.as_str()).collect();
                assert_eq!(variant_names, vec!["In", "Out", "Bidi"]);
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
    /// Visits members in declaration order via `visit_structure_member_root_exprs`,
    /// which yields `Param` defaults before `Let` values within the same structure.
    /// A `Param` default carrying an `EnumAccess` will therefore be returned if it
    /// appears before any `Let` with an `EnumAccess`.
    fn find_first_enum_access(module: &ParsedModule) -> Option<(String, String)> {
        let mut result = None;
        crate::visit_structure_member_root_exprs(module, |expr| {
            if result.is_none()
                && let ExprKind::EnumAccess { type_name, variant } = &expr.kind
            {
                result = Some((type_name.clone(), variant.clone()));
            }
        });
        result
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
    /// Note: the API accepts `&[&'a str]` (source-lifetime bound, task 4108);
    /// non-`'static` borrowed names are accepted and covered by
    /// `parse_with_prelude_enums_accepts_non_static_borrowed_names`.  The
    /// no-allocation guarantee is a manual profiling check (per task description),
    /// not encoded here.
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

        // Collect all EnumAccess root-expr values from S2 via the shared visitor.
        let mut accesses: Vec<(String, String)> = Vec::new();
        crate::visit_structure_member_root_exprs(&module2, |expr| {
            if let ExprKind::EnumAccess { type_name, variant } = &expr.kind {
                accesses.push((type_name.clone(), variant.clone()));
            }
        });
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

    /// (e) Compile-level regression guard for the lifetime relaxation (task 4108).
    /// Proves that `parse_with_prelude_enums` accepts a name slice whose
    /// elements are borrowed from a non-`'static` local allocation.
    ///
    /// Under the OLD `&[&'static str]` bound this test DOES NOT COMPILE:
    /// the compiler rejects `&names` because `names: Vec<&str>` borrows from
    /// `owned: Vec<String>` — a local, non-`'static` value.  After step-2
    /// relaxes the bound to `&[&'a str]` the test compiles and passes.
    ///
    /// Runtime behavior (EnumAccess disambiguation for a non-`'static` name)
    /// is already covered by `parse_with_prelude_enums_resolves_prelude_only_enum`
    /// with the same source pattern; this test's sole new capability under test
    /// is accepting a non-`'static` borrow.
    #[test]
    fn parse_with_prelude_enums_accepts_non_static_borrowed_names() {
        let owned: Vec<String> = vec!["Foo".to_string()];
        let names: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        let module = parse_with_prelude_enums(
            "structure S { let v = Foo.Bar }",
            ModulePath::single("test_nonstatic"),
            &names,
        );
        assert!(
            module.errors.is_empty(),
            "parse errors: {:?}",
            module.errors
        );
        let (type_name, variant) =
            find_first_enum_access(&module).expect("expected EnumAccess from non-static names");
        assert_eq!(type_name, "Foo");
        assert_eq!(variant, "Bar");
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
        let source = "fn area(w: Length, h: Length) -> Length { w * h }";
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
            matches!(&f.params[0].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Length")
        );
        assert_eq!(f.params[1].name, "h");
        assert!(
            matches!(&f.params[1].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Length")
        );
        assert!(f.return_type.is_some());
        assert!(
            matches!(&f.return_type.as_ref().unwrap().kind, TypeExprKind::Named { name, .. } if name == "Length")
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
        // NOTE: we use `: BoltSet` to specify a connector_type before the brace
        // block, making `{` unambiguously the start of connect_body.  Without
        // the connector_type, the new variant_construction GLR fork (task α,
        // data-carrying-enums) keeps both a variant_construction fork and the
        // connect_body fork alive after `b {`; even though `>=` immediately
        // kills the variant_construction fork, GLR error recovery may orphan
        // `{ … }` as a member-level ERROR node rather than a connect_body,
        // causing `find_node_by_kind("connect_body")` to fail.  The connector
        // type `: BoltSet` consumes the `b :` prefix so the `{` is unambiguous.
        let errors = lower_body_with_errors(
            "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { >= } }",
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
        let source = "constraint def Eq { param x: Length  x > 0 }";
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
        let source = "constraint def Eq { param x: Length  x > 0 }";
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
            "structure S { port a : in T { /* comment */ param x: Length = 1 } }",
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
        let source = "structure S { port a : in T { param x: Length = 1 }  sub b = T() }";
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
            "constraint def Eq { /* comment */ param x: Length  x > 0 }",
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
        let source = "structure S { param x: Length = 1  port a : in T { param y: Length = 2 } }";
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
        let source = "/* comment */\nstructure S { param x: Length = 1 }";
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
        let src = "/// A bracket for mounting.\nstructure Bracket {\n  param w: Length = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("A bracket for mounting."));
    }

    #[test]
    fn multi_line_doc_comment_joined() {
        let src = "/// Line one.\n/// Line two.\nstructure S {\n  param x: Length = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert_eq!(decl.doc.as_deref(), Some("Line one.\nLine two."));
    }

    #[test]
    fn no_doc_comment_yields_none() {
        let src = "structure S {\n  param x: Length = 1\n}";
        let module = parse(src, ModulePath::single("test"));
        let decl = match &module.declarations[0] {
            Declaration::Structure(s) => s,
            other => panic!("expected Structure, got: {other:?}"),
        };
        assert!(decl.doc.is_none());
    }

    #[test]
    fn regular_comment_not_treated_as_doc() {
        let src = "// Just a comment\nstructure S {\n  param x: Length = 1\n}";
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
        let src = "/// Compute area.\nfn area(w: Length, h: Length) -> Length { w * h }";
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
        let src = "/// A rigid body.\ntrait Rigid {\n  param mass: Length\n}";
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

    // ── Unit tests for decode_string_escapes (suggestion 2 coverage) ──────────

    /// An unrecognized escape sequence (e.g. `\r`, `\0`) drops the backslash
    /// and keeps only the character.  This pins the "lenient" behavior
    /// documented in the `decode_string_escapes` doc-comment.
    ///
    /// Concretely: `\X` where X is not `n`, `t`, `\\`, or `"` → emit X,
    /// drop the backslash.  This is data-lossy but intentional for α.
    #[test]
    fn decode_string_escapes_unknown_escape_drops_backslash() {
        // `\r` → 'r' (backslash dropped, character kept)
        assert_eq!(decode_string_escapes("x\\ry"), "xry");
        // `\0` → '0'
        assert_eq!(decode_string_escapes("\\0z"), "0z");
        // `\s` → 's'
        assert_eq!(decode_string_escapes("a\\sb"), "asb");
    }

    /// A lone `\` at the very end of a chunk (no following character) is
    /// silently dropped — the `None => {}` branch in `decode_string_escapes`.
    ///
    /// This is reachable on highly-malformed input (e.g. the external scanner
    /// consumed a lone `\` at EOF inside a string literal).  Pinning the
    /// behavior here prevents a silent regression if the semantics ever change.
    #[test]
    fn decode_string_escapes_trailing_backslash_is_silently_dropped() {
        // A chunk ending with a lone backslash: the backslash disappears.
        assert_eq!(decode_string_escapes("x\\"), "x");
        assert_eq!(decode_string_escapes("\\"), "");
    }

    // ── Unit test for lower_interpolated_string robustness (suggestion 3) ─────

    /// Directly exercises `lower_interpolated_string` with a malformed empty
    /// hole `{}` to verify the function-level robustness fix, *bypassing*
    /// `check_and_lower!` (which fires at the `let_declaration` level and
    /// prevents the function from being called in the full-pipeline path).
    ///
    /// Verifies that:
    /// 1. The function returns `Some(...)` — the string is NOT silently dropped.
    /// 2. A diagnostic is emitted for the MISSING-expr hole.
    /// 3. The surviving literal parts remain in the result.
    #[test]
    fn lower_interpolated_string_malformed_hole_produces_diagnostic() {
        let source = r#"structure S { let v = "x {} y" }"#;
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("Error loading Reify grammar");
        let tree = ts_parser.parse(source, None).expect("Failed to parse");
        let root = tree.root_node();

        // Find the interpolated_string node (has_error due to the empty hole).
        let interp_node = find_node_by_kind(root, "interpolated_string")
            .expect("interpolated_string node must be present in CST");
        assert!(
            interp_node.has_error(),
            "the interpolated_string node must have has_error=true for this test to be meaningful"
        );

        // Call lower_interpolated_string directly, bypassing check_and_lower!
        let lowering = Lowering::new(source);
        let result = lowering.lower_interpolated_string(interp_node);
        let errors = lowering.errors.into_inner();

        // The string must NOT be silently dropped (Some returned, not None).
        let expr = result
            .expect("lower_interpolated_string must return Some even for a malformed hole");

        // At least one diagnostic for the bad hole.
        assert!(
            !errors.is_empty(),
            "expected at least one diagnostic for MISSING-expr hole, got none"
        );

        // The surrounding literal chunks survive; the bad hole is skipped.
        match &expr.kind {
            ExprKind::InterpolatedString(parts) => {
                let literal_count = parts
                    .iter()
                    .filter(|p| matches!(p, StringPart::Literal(_)))
                    .count();
                assert!(
                    literal_count >= 2,
                    "expected at least 2 surviving literal parts, got: {:?}",
                    parts
                );
            }
            other => panic!("expected InterpolatedString, got {:?}", other),
        }
    }

    /// Robustness guard: `qualified_type_recovery_base` must return a flat,
    /// bounded `Named` over the whole-node text — NOT dispatch back through
    /// `lower_type_expr_node` — so that the error-recovery (`None`) arm of
    /// `lower_qualified_type` does not recurse into itself and overflow the
    /// stack in release builds.
    ///
    /// The source is valid (the `qualified_type` node is real); we pass that
    /// real node to `qualified_type_recovery_base` to exercise the helper on
    /// a concrete CST node without needing an unreachable baseless node.
    #[test]
    fn qualified_type_recovery_base_is_bounded_named() {
        let source = "structure def S { param m : Coupling<Prismatic>::MotionValue }";

        // Parse with the raw tree-sitter API to get the CST directly.
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("set_language failed");
        let tree = ts_parser
            .parse(source, None)
            .expect("parse returned None");
        let root = tree.root_node();

        // Walk the CST to locate the qualified_type node (reuses the helper
        // already defined in this test module at line 5764).
        let qnode = find_node_by_kind(root, "qualified_type")
            .expect("expected a qualified_type node in the CST");

        // Build the lowering context.
        let lowering = Lowering::new(source);

        // Call the recovery helper directly.  Asserting a flat Named over the
        // whole-node text (not a QualifiedAssoc) is the discriminating signal
        // that the helper does NOT re-dispatch through lower_type_expr_node.
        let result = lowering.qualified_type_recovery_base(qnode);

        match result.kind {
            TypeExprKind::Named { name, type_args } => {
                assert_eq!(
                    name, "Coupling<Prismatic>::MotionValue",
                    "recovery base must be the raw whole-node text"
                );
                assert!(
                    type_args.is_empty(),
                    "recovery base must have empty type_args (no parsing), got: {:?}",
                    type_args
                );
            }
            other => panic!(
                "expected TypeExprKind::Named (bounded flat recovery), got {:?}",
                other
            ),
        }
    }

    // ── `classify_unit_op` — the non-happy-path arms of `lower_unit_expr` ─────
    //
    // Task #5784 (angle-units leaf κ).  These pin the CLASSIFICATION and the
    // verbatim operator text handed to the diagnostic; the message wording itself
    // is built at the single call site in `lower_unit_expr`.
    //
    // Both leftover arms are DEFENSIVE — no probed source reaches `Unrecognized`
    // or `Missing` now that `strip_unit_op_comments` runs first (see the
    // [`UnitOp`] doc for why they still must diagnose rather than return a bare
    // `None`).  Tests are therefore their only observation, and these cover just
    // one half of it: WHICH ARM a slice lands in.  What the call site does with
    // that arm — that a diagnostic fires at all and names the operator, the span
    // it attaches, and the silence of `Missing` — is pinned by `unit_op_seam_*`
    // below.  Deleting either group as
    // "dead code" restores the silent-member-drop hazard unobserved.

    #[test]
    fn classify_unit_op_maps_both_mul_spellings_to_one_operator() {
        assert_eq!(classify_unit_op("*"), UnitOp::Mul);
        assert_eq!(
            classify_unit_op("·"),
            UnitOp::Mul,
            "U+00B7 MIDDLE DOT is a second spelling of `*`, not a distinct operator"
        );
        assert_eq!(classify_unit_op("/"), UnitOp::Div);
    }

    #[test]
    fn classify_unit_op_treats_an_empty_slice_as_a_missing_operator() {
        // An empty slice means error recovery spliced the operands together, so
        // the tree already carries the real syntax error.  `Missing` is what tells
        // `lower_unit_expr` to stay quiet rather than emit a second, confusingly
        // worded "unrecognized unit operator ``".
        assert_eq!(classify_unit_op(""), UnitOp::Missing);
        assert_eq!(classify_unit_op("   "), UnitOp::Missing);
    }

    #[test]
    fn classify_unit_op_carries_an_unknown_operator_verbatim() {
        // Whatever the caller names in its diagnostic must be the operator the
        // user actually wrote — trimmed, never truncated or normalised.
        assert_eq!(classify_unit_op("×"), UnitOp::Unrecognized("×"));
        assert_eq!(classify_unit_op(" ⋅ "), UnitOp::Unrecognized("⋅"));
        assert_eq!(classify_unit_op("**"), UnitOp::Unrecognized("**"));
    }

    // ── `strip_unit_op_comments` — comments are `extras`, so they land in the
    //    operator slice.  Offsets here are the real ones for the cited sources
    //    (the caller passes ABSOLUTE byte ranges plus the slice's own start).

    #[test]
    fn strip_unit_op_comments_borrows_when_there_is_nothing_to_cut() {
        // The overwhelmingly common path: no comment, no allocation.
        let out = strip_unit_op_comments("*", 24, &[]);
        assert!(matches!(out, Cow::Borrowed("*")));
        assert_eq!(strip_unit_op_comments("·", 24, &[]).as_ref(), "·");
    }

    #[test]
    fn strip_unit_op_comments_leaves_the_bare_operator() {
        // `structure S { let x = 5N/*c*/*m }` — slice `/*c*/*` starts at byte 24,
        // the block_comment spans 24..29, so the residue is the trailing `*`.
        assert_eq!(
            strip_unit_op_comments("/*c*/*", 24, &[(24, 29)]).as_ref(),
            "*",
            "a comment-bearing Mul must classify as Mul, not as an unrecognized \
             operator — the grammar accepted this source with no ERROR node"
        );
        // The `·` and `/` spellings take the identical path.
        assert_eq!(
            strip_unit_op_comments("/*c*/·", 24, &[(24, 29)]).as_ref(),
            "·"
        );
        assert_eq!(
            strip_unit_op_comments("/*c*//", 24, &[(24, 29)]).as_ref(),
            "/"
        );
    }

    #[test]
    fn strip_unit_op_comments_handles_several_comments_before_the_operator() {
        // `structure def S { let x = 5N/*a*//*b*/*m }` — measured: a clean parse
        // whose `unit_expr` carries TWO block_comment children, slice
        // `/*a*//*b*/*` at 28..39 with comments at 28..33 and 33..38.
        assert_eq!(
            strip_unit_op_comments("/*a*//*b*/*", 28, &[(28, 33), (33, 38)]).as_ref(),
            "*",
            "every comment span must come out, not just the first"
        );
    }

    #[test]
    fn strip_unit_op_comments_skips_ranges_it_cannot_apply() {
        // Defensive: a cut outside the slice, a descending pair, and one running
        // past the end are each SKIPPED, never panicked on.  The residue then
        // still carries the comment text and classifies as `Unrecognized` — loud,
        // which is the correct degradation for a CST shape we did not predict.
        let slice = "/*c*/*";
        assert_eq!(
            strip_unit_op_comments(slice, 24, &[(0, 4)]).as_ref(),
            slice,
            "a cut before the slice must not be re-based onto it"
        );
        assert_eq!(
            strip_unit_op_comments(slice, 24, &[(29, 24)]).as_ref(),
            slice,
            "a descending range must be skipped"
        );
        assert_eq!(
            strip_unit_op_comments(slice, 24, &[(24, 999)]).as_ref(),
            slice,
            "a range running past the slice must be skipped"
        );
    }

    #[test]
    fn strip_unit_op_comments_only_ever_shrinks_toward_a_real_operator() {
        // A slice that is NOTHING but a comment reduces to `Missing`, not to a
        // second diagnostic: the operator token is genuinely absent, so error
        // recovery already put an ERROR/MISSING node in the tree for
        // `check_and_lower!` to report.
        let residue = strip_unit_op_comments("/*c*/", 24, &[(24, 29)]);
        assert_eq!(residue.as_ref(), "");
        assert_eq!(classify_unit_op(&residue), UnitOp::Missing);
    }

    // ── `collect_unit_op_comment_spans` — WHICH comments feed the excision ────
    //
    // #5784 amendment pass.  `strip_unit_op_comments` above is pinned against
    // hand-written ranges; these pin the step that PRODUCES those ranges from a
    // real CST, which is where the excision path's correctness actually rests.
    // The sweep walks the subtree rather than the node's direct children,
    // because an `extra`'s attachment point belongs to the generated parser, not
    // to the grammar rule — see the function's doc.

    /// Parse `source`, find the outer `unit_expr`, and return its operator
    /// slice's start offset, the slice itself, and the spans the sweep collects
    /// from it.
    fn unit_op_comment_spans_of(source: &str) -> (usize, &str, Vec<(usize, usize)>) {
        let tree = unit_op_seam_tree(source);
        assert!(
            !tree.root_node().has_error(),
            "`{source}` must parse CLEAN — a probe that errors would be \
             measuring error recovery, not comment attachment"
        );
        let unit_expr = find_node_by_kind(tree.root_node(), "unit_expr")
            .expect("expected a unit_expr node in the CST");
        let left = unit_expr
            .child_by_field_name("left")
            .expect("expected a `left` operand");
        let right = unit_expr
            .child_by_field_name("right")
            .expect("expected a `right` operand");
        let (op_start, op_end) = (left.end_byte(), right.start_byte());
        (
            op_start,
            &source[op_start..op_end],
            collect_unit_op_comment_spans(unit_expr, op_start, op_end),
        )
    }

    #[test]
    fn collect_unit_op_comment_spans_finds_every_comment_in_the_slice_in_order() {
        let source = "structure def S { let x = 5N/*c*/*m }";
        let (op_start, slice, spans) = unit_op_comment_spans_of(source);
        assert_eq!(slice, "/*c*/*");
        assert_eq!(spans.len(), 1, "one comment, one span; got {spans:?}");
        assert_eq!(&source[spans[0].0..spans[0].1], "/*c*/");
        assert_eq!(
            strip_unit_op_comments(slice, op_start, &spans).as_ref(),
            "*",
            "the collected spans must reduce the slice to the operator the CST \
             plainly shows"
        );

        // Two comments: ascending and non-overlapping, which is what
        // `strip_unit_op_comments` requires of `cuts`.
        let source = "structure def S { let x = 5N/*a*//*b*/*m }";
        let (op_start, slice, spans) = unit_op_comment_spans_of(source);
        assert_eq!(slice, "/*a*//*b*/*");
        assert_eq!(spans.len(), 2, "two comments, two spans; got {spans:?}");
        assert!(
            spans[0].1 <= spans[1].0,
            "spans must come back in ascending, non-overlapping source order; \
             got {spans:?}"
        );
        assert_eq!(
            strip_unit_op_comments(slice, op_start, &spans).as_ref(),
            "*"
        );
    }

    /// The source below is the one probed shape whose comment is attached BELOW
    /// the outer `unit_expr` — measured on a clean parse:
    ///
    /// ```text
    ///   5(m/*c*/)*s  →  unit_expr(unit_expr(unit_expr(unit_name), block_comment),
    ///                             unit_expr(unit_name))
    /// ```
    const NESTED_COMMENT_SOURCE: &str = "structure def S { let x = 5(m/*c*/)*s }";

    #[test]
    fn collect_unit_op_comment_spans_ignores_a_comment_outside_the_slice() {
        // Its comment sits inside the LEFT operand, so the operator slice is
        // already exactly `*` and excising anything would corrupt it.  Two
        // independent guards keep it out — the sibling prune skips a subtree
        // disjoint from the slice, and the range filter rejects the comment
        // itself — and this pins the OUTCOME, which must survive either being
        // rewritten.
        let (_op_start, slice, spans) = unit_op_comment_spans_of(NESTED_COMMENT_SOURCE);
        assert_eq!(slice, "*");
        assert!(
            spans.is_empty(),
            "a comment outside `[op_start, op_end)` must not be cut, however \
             deep the walk goes; got {spans:?}"
        );
    }

    #[test]
    fn collect_unit_op_comment_spans_reaches_a_comment_attached_below_the_node() {
        // THE DEPTH CLAIM, and the only test that bites on it: this is the sole
        // shape in which a real parse attaches a comment to a DESCENDANT of the
        // `unit_expr` rather than to it directly, so a direct-children sweep
        // returns nothing here while the subtree walk finds it.
        //
        // The range passed is the WHOLE node, not the operator slice — widened
        // deliberately, because today no accepted source puts a depth-attached
        // comment INSIDE a slice.  That is a fact about the generated parser's
        // current `extras` placement, not about the grammar, which is exactly why
        // the sweep must not assume it: if the attachment point moves, this
        // function keeps working and no spurious `unrecognized unit operator`
        // reaches a user.
        let tree = unit_op_seam_tree(NESTED_COMMENT_SOURCE);
        let unit_expr = find_node_by_kind(tree.root_node(), "unit_expr")
            .expect("expected a unit_expr node in the CST");
        let comment = find_node_by_kind(unit_expr, "block_comment")
            .expect("fixture drift: expected a block_comment somewhere under the unit_expr");
        assert_ne!(
            comment.parent().map(|p| p.id()),
            Some(unit_expr.id()),
            "fixture drift: this test means to observe a comment attached BELOW \
             the outer `unit_expr`; it is now a direct child, so the source no \
             longer exercises the depth the walk exists for"
        );

        let spans =
            collect_unit_op_comment_spans(unit_expr, unit_expr.start_byte(), unit_expr.end_byte());

        assert_eq!(
            spans,
            vec![(comment.start_byte(), comment.end_byte())],
            "the sweep must reach a comment attached below the node it is given"
        );
    }
    // ── The CALL SITE: `Lowering::unit_expr_from_classified_op` ───────────────
    //
    // #5784 amendment pass.  The `classify_unit_op_*` tests above stop at the
    // classification; nothing observed what the caller then DOES with a dropping
    // arm — not that a diagnostic fires at all, not the span it attaches, not
    // the SILENCE of `Missing`.  Both arms are unreachable from source (the scanner
    // emits only `*`, `·` and `/`, and comments are excised before classifying),
    // so the classification is the synthetic half here while the NODE stays real:
    // the span assertion is then a genuine claim about which construct an editor
    // underlines.  Same shape as `qualified_type_recovery_base_is_bounded_named`
    // above — a real CST node, a helper called directly.

    /// The source every seam test below drives, and the `unit_expr` it targets.
    const UNIT_OP_SEAM_SOURCE: &str = "structure def S { let x = 5N*m }";
    const UNIT_OP_SEAM_UNIT: &str = "N*m";

    /// Parse `source` with the raw tree-sitter API so the CST is reachable.
    fn unit_op_seam_tree(source: &str) -> tree_sitter::Tree {
        let mut ts_parser = tree_sitter::Parser::new();
        ts_parser
            .set_language(&tree_sitter_reify::language().into())
            .expect("set_language failed");
        ts_parser.parse(source, None).expect("parse returned None")
    }

    /// The two operands every seam test passes in — named so an assertion that
    /// they came back in ORDER is readable.
    fn unit_op_seam_operands() -> (UnitExpr, UnitExpr) {
        (
            UnitExpr::Unit("N".to_string()),
            UnitExpr::Unit("m".to_string()),
        )
    }

    #[test]
    fn unit_op_seam_unrecognized_names_the_operator_and_spans_the_expression() {
        let tree = unit_op_seam_tree(UNIT_OP_SEAM_SOURCE);
        let unit_expr = find_node_by_kind(tree.root_node(), "unit_expr")
            .expect("expected a unit_expr node in the CST");
        assert_eq!(
            &UNIT_OP_SEAM_SOURCE[unit_expr.start_byte()..unit_expr.end_byte()],
            UNIT_OP_SEAM_UNIT,
            "fixture drift: this test means to span the WHOLE compound unit, so \
             the node it found must be the outer `unit_expr`, not an operand"
        );
        let lowering = Lowering::new(UNIT_OP_SEAM_SOURCE);
        let (left, right) = unit_op_seam_operands();

        // `/*c*/*` is the residue shape κ used to reject before comment excision
        // landed — kept as the probe because it is the one operator text this
        // arm has ever really been handed.
        let lowered = lowering.unit_expr_from_classified_op(
            UnitOp::Unrecognized("/*c*/*"),
            left,
            right,
            unit_expr,
        );

        assert_eq!(
            lowered, None,
            "an unrecognized operator must drop the member — but see the \
             diagnostic below: dropping it SILENTLY is the INV-SF-7 shape"
        );
        let errors = lowering.errors.borrow();
        let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        // The two SUBSTANTIVE claims, asserted separately: exactly one
        // diagnostic, and it quotes the operator verbatim.  Deliberately NOT a
        // full-sentence equality against the message — this arm is unreachable
        // from any source the grammar accepts, so the exact wording is a string
        // no user can currently observe.  Pinning it would red this seam on a
        // reword that changes no behaviour, while both claims below survive one
        // (#5784 amendment pass).
        assert_eq!(
            messages.len(),
            1,
            "the drop must produce exactly ONE diagnostic — a second here means \
             the caller is also reporting the same node through \
             `check_and_lower!`; got {messages:?}"
        );
        assert!(
            messages[0].contains("/*c*/*"),
            "the diagnostic must quote the rejected operator VERBATIM, so the \
             user can see WHICH operator was not understood; got {:?}",
            messages[0]
        );
        let span = errors[0].span;
        assert_eq!(
            (span.start, span.end),
            (unit_expr.start_byte() as u32, unit_expr.end_byte() as u32),
            "the diagnostic must underline the whole `{UNIT_OP_SEAM_UNIT}` it \
             rejected, not the operator alone and not the file"
        );
    }

    #[test]
    fn unit_op_seam_missing_drops_the_member_without_a_second_diagnostic() {
        let tree = unit_op_seam_tree(UNIT_OP_SEAM_SOURCE);
        let unit_expr = find_node_by_kind(tree.root_node(), "unit_expr")
            .expect("expected a unit_expr node in the CST");
        let lowering = Lowering::new(UNIT_OP_SEAM_SOURCE);
        let (left, right) = unit_op_seam_operands();

        let lowered =
            lowering.unit_expr_from_classified_op(UnitOp::Missing, left, right, unit_expr);

        assert_eq!(lowered, None, "a missing operator cannot build a UnitExpr");
        assert!(
            lowering.errors.borrow().is_empty(),
            "`Missing` must stay SILENT: error recovery spliced the operands \
             together, so `check_and_lower!` already reported the ERROR/MISSING \
             node — a diagnostic here would be the second one for one mistake, \
             got {:?}",
            lowering.errors.borrow()
        );
    }

    #[test]
    fn unit_op_seam_builds_mul_and_div_in_operand_order() {
        let tree = unit_op_seam_tree(UNIT_OP_SEAM_SOURCE);
        let unit_expr = find_node_by_kind(tree.root_node(), "unit_expr")
            .expect("expected a unit_expr node in the CST");
        let lowering = Lowering::new(UNIT_OP_SEAM_SOURCE);
        let (left, right) = unit_op_seam_operands();
        let expected_left = Box::new(UnitExpr::Unit("N".to_string()));
        let expected_right = Box::new(UnitExpr::Unit("m".to_string()));

        assert_eq!(
            lowering.unit_expr_from_classified_op(
                UnitOp::Mul,
                left.clone(),
                right.clone(),
                unit_expr
            ),
            Some(UnitExpr::Mul(
                expected_left.clone(),
                expected_right.clone()
            )),
            "`Mul` must keep the operands in source order — swapping them is \
             invisible to every commutative-looking end-to-end assertion"
        );
        assert_eq!(
            lowering.unit_expr_from_classified_op(UnitOp::Div, left, right, unit_expr),
            Some(UnitExpr::Div(expected_left, expected_right)),
            "`Div` is NOT commutative: numerator left, denominator right"
        );
        assert!(
            lowering.errors.borrow().is_empty(),
            "a recognised operator must not diagnose, got {:?}",
            lowering.errors.borrow()
        );
    }
}
