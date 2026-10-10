//! The items a provider gates on one feature, read from its module tree.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, Field, Ident, ImplItem, Item, ItemMod, Lit, Meta, Token, TraitItem,
    Type, UseTree, Visibility,
};

#[derive(Debug)]
pub enum ScanError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    UnresolvedModule {
        declared_in: PathBuf,
        line: usize,
        module: String,
    },
    UnsupportedPath {
        declared_in: PathBuf,
        line: usize,
    },
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Parse { path, message } => {
                write!(f, "cannot parse {}: {message}", path.display())
            }
            Self::UnresolvedModule {
                declared_in,
                line,
                module,
            } => write!(
                f,
                "{}:{line}: no file for `mod {module};`",
                declared_in.display()
            ),
            Self::UnsupportedPath { declared_in, line } => write!(
                f,
                "{}:{line}: #[path] on or inside an inline module is not supported",
                declared_in.display()
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeViolation {
    pub file: PathBuf,
    pub line: usize,
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    CfgAttr,
    TraitImplForUngatedSelf { self_ty: String },
    GatedItemInUngatedTraitImpl { item: String },
    GlobReexport,
    ItemMacroInvocation,
}

#[derive(Debug)]
pub struct GatedSurface {
    pub evidence: BTreeSet<String>,
    pub violations: Vec<ShapeViolation>,
}

impl GatedSurface {
    pub fn scan(lib_root: &Path, feature: &str) -> Result<GatedSurface, ScanError> {
        let mut walker = Walker {
            feature,
            entries: Vec::new(),
        };
        let mod_dir = lib_root.parent().unwrap_or(Path::new("")).to_path_buf();
        walker.walk_file(lib_root, mod_dir, false)?;
        Ok(classify(&walker.entries))
    }
}

/// One surface position of the provider: an item, or a named member of one.
#[derive(Debug)]
struct SurfaceEntry {
    kind: EntryKind,
    name: Option<String>,
    own_gate: bool,
    inherited_gate: bool,
    public: bool,
    file: PathBuf,
    line: usize,
}

impl SurfaceEntry {
    fn gated(&self) -> bool {
        self.own_gate || self.inherited_gate
    }
}

#[derive(Debug)]
enum EntryKind {
    /// struct / enum / union / type alias / trait: may be an impl's Self.
    TypeItem,
    /// fn / const / static / mod / extern crate.
    NamedItem,
    MacroRules,
    MacroInvocation,
    Use {
        leaves: Vec<String>,
        glob: bool,
    },
    Impl {
        trait_impl: bool,
        self_ty: Option<String>,
    },
    ImplItem {
        in_trait_impl: bool,
        self_ty: Option<String>,
    },
    TraitItem,
    Field,
    Variant,
    Other,
}

/// Where the items being walked live, and where their `mod x;` children resolve.
struct ModuleCtx {
    file: PathBuf,
    mod_dir: PathBuf,
    gated: bool,
    inline: bool,
}

/// What the walk reads off one surface position before gating it.
struct Facts<'a> {
    kind: EntryKind,
    name: Option<String>,
    attrs: &'a [Attribute],
    public: bool,
}

struct Walker<'f> {
    feature: &'f str,
    entries: Vec<SurfaceEntry>,
}

impl Walker<'_> {
    fn walk_file(&mut self, file: &Path, mod_dir: PathBuf, gated: bool) -> Result<(), ScanError> {
        let source = fs::read_to_string(file).map_err(|source| ScanError::Io {
            path: file.to_path_buf(),
            source,
        })?;
        let parsed = syn::parse_file(&source).map_err(|e| ScanError::Parse {
            path: file.to_path_buf(),
            message: e.to_string(),
        })?;
        let ctx = ModuleCtx {
            file: file.to_path_buf(),
            mod_dir,
            gated,
            inline: false,
        };
        parsed
            .items
            .iter()
            .try_for_each(|item| self.walk_item(item, &ctx))
    }

    fn walk_item(&mut self, item: &Item, ctx: &ModuleCtx) -> Result<(), ScanError> {
        let facts = item_facts(item);
        let public = facts.public;
        let gated = self.record(facts, ctx.gated, &ctx.file, item);
        match item {
            Item::Mod(module) => self.walk_mod(module, ctx, gated)?,
            Item::Impl(imp) => {
                let self_ty = self_type_name(&imp.self_ty);
                for member in &imp.items {
                    let facts = impl_item_facts(member, imp.trait_.is_some(), self_ty.clone());
                    self.record(facts, gated, &ctx.file, member);
                }
            }
            Item::Trait(tr) => {
                for member in &tr.items {
                    self.record(trait_item_facts(member, public), gated, &ctx.file, member);
                }
            }
            Item::Struct(st) => self.walk_fields(&st.fields, gated, &ctx.file),
            Item::Union(un) => self.walk_fields(&un.fields.named, gated, &ctx.file),
            Item::Enum(en) => {
                for variant in &en.variants {
                    let facts = Facts {
                        kind: EntryKind::Variant,
                        name: Some(variant.ident.unraw().to_string()),
                        attrs: &variant.attrs,
                        public,
                    };
                    self.record(facts, gated, &ctx.file, variant);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn walk_mod(
        &mut self,
        module: &ItemMod,
        ctx: &ModuleCtx,
        gated: bool,
    ) -> Result<(), ScanError> {
        let name = module.ident.unraw().to_string();
        let path_attr = path_attribute(&module.attrs);
        if path_attr.is_some() && (ctx.inline || module.content.is_some()) {
            return Err(ScanError::UnsupportedPath {
                declared_in: ctx.file.clone(),
                line: line_of(module),
            });
        }
        if let Some((_, items)) = &module.content {
            let inner = ModuleCtx {
                file: ctx.file.clone(),
                mod_dir: ctx.mod_dir.join(&name),
                gated,
                inline: true,
            };
            return items
                .iter()
                .try_for_each(|item| self.walk_item(item, &inner));
        }
        let unresolved = || ScanError::UnresolvedModule {
            declared_in: ctx.file.clone(),
            line: line_of(module),
            module: name.clone(),
        };
        let (file, mod_dir) = match path_attr {
            Some(rel) => {
                let file = ctx.file.parent().unwrap_or(Path::new("")).join(rel);
                let mod_dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
                (file, mod_dir)
            }
            None => {
                let file = module_file(&ctx.mod_dir, &name).ok_or_else(unresolved)?;
                let mod_dir = child_mod_dir(&file);
                (file, mod_dir)
            }
        };
        if !file.is_file() {
            return Err(unresolved());
        }
        self.walk_file(&file, mod_dir, gated)
    }

    fn walk_fields<'a>(
        &mut self,
        fields: impl IntoIterator<Item = &'a Field>,
        parent_gated: bool,
        file: &Path,
    ) {
        for field in fields {
            if let Some(ident) = &field.ident {
                let facts = Facts {
                    kind: EntryKind::Field,
                    name: Some(ident.unraw().to_string()),
                    attrs: &field.attrs,
                    public: is_public(&field.vis),
                };
                self.record(facts, parent_gated, file, field);
            }
        }
    }

    /// Gates one position (own cfg OR the enclosing gate), records it, and
    /// returns its gate for the positions nested inside it.
    fn record(
        &mut self,
        facts: Facts,
        parent_gated: bool,
        file: &Path,
        node: &impl Spanned,
    ) -> bool {
        let entry = SurfaceEntry {
            kind: facts.kind,
            name: facts.name,
            own_gate: self.gates(facts.attrs),
            inherited_gate: parent_gated,
            public: facts.public,
            file: file.to_path_buf(),
            line: line_of(node),
        };
        let gated = entry.gated();
        self.entries.push(entry);
        gated
    }

    fn gates(&self, attrs: &[Attribute]) -> bool {
        attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg"))
            .filter_map(|attr| attr.parse_args::<Meta>().ok())
            .any(|predicate| positively_mentions(&predicate, self.feature))
    }
}

/// THE gating predicate: does this cfg predicate turn on with `feature`
/// rather than off (a mention under `not(...)` flips polarity)?
fn positively_mentions(meta: &Meta, feature: &str) -> bool {
    mentions_with_polarity(meta, feature, true)
}

fn mentions_with_polarity(meta: &Meta, feature: &str, positive: bool) -> bool {
    match meta {
        Meta::NameValue(nv) => {
            positive && nv.path.is_ident("feature") && is_str_literal(&nv.value, feature)
        }
        Meta::List(list) => {
            let positive = if list.path.is_ident("not") {
                !positive
            } else if list.path.is_ident("any") || list.path.is_ident("all") {
                positive
            } else {
                return false;
            };
            list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .is_ok_and(|children| {
                    children
                        .iter()
                        .any(|child| mentions_with_polarity(child, feature, positive))
                })
        }
        Meta::Path(_) => false,
    }
}

fn is_str_literal(expr: &Expr, expected: &str) -> bool {
    matches!(expr, Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) if s.value() == expected)
}

fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

fn line_of(node: &impl Spanned) -> usize {
    node.span().start().line
}

fn path_attribute(attrs: &[Attribute]) -> Option<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("path"))
        .find_map(|attr| match &attr.meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
}

/// `mod name;` declared with children resolving in `mod_dir`: `name.rs` or `name/mod.rs`.
fn module_file(mod_dir: &Path, name: &str) -> Option<PathBuf> {
    [
        mod_dir.join(format!("{name}.rs")),
        mod_dir.join(name).join("mod.rs"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// Where a module file's own `mod x;` children resolve: beside a `mod.rs`,
/// under `<stem>/` for any other file.
fn child_mod_dir(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new(""));
    match file.file_stem().and_then(|stem| stem.to_str()) {
        Some("mod") | None => dir.to_path_buf(),
        Some(stem) => dir.join(stem),
    }
}

fn unraw(ident: &Ident) -> Option<String> {
    Some(ident.unraw().to_string())
}

fn item_facts(item: &Item) -> Facts<'_> {
    let named = |kind, ident: &Ident, attrs, vis: &Visibility| Facts {
        kind,
        name: unraw(ident),
        attrs,
        public: is_public(vis),
    };
    let unnamed = |kind, attrs| Facts {
        kind,
        name: None,
        attrs,
        public: false,
    };
    match item {
        Item::Const(i) => named(EntryKind::NamedItem, &i.ident, &i.attrs, &i.vis),
        Item::Enum(i) => named(EntryKind::TypeItem, &i.ident, &i.attrs, &i.vis),
        Item::ExternCrate(i) => {
            let ident = i.rename.as_ref().map_or(&i.ident, |(_, rename)| rename);
            named(EntryKind::NamedItem, ident, &i.attrs, &i.vis)
        }
        Item::Fn(i) => named(EntryKind::NamedItem, &i.sig.ident, &i.attrs, &i.vis),
        Item::Impl(i) => {
            let kind = EntryKind::Impl {
                trait_impl: i.trait_.is_some(),
                self_ty: self_type_name(&i.self_ty),
            };
            unnamed(kind, &i.attrs)
        }
        Item::Macro(i) => match &i.ident {
            Some(ident) => Facts {
                kind: EntryKind::MacroRules,
                name: unraw(ident),
                attrs: &i.attrs,
                public: false,
            },
            None => unnamed(EntryKind::MacroInvocation, &i.attrs),
        },
        Item::Mod(i) => named(EntryKind::NamedItem, &i.ident, &i.attrs, &i.vis),
        Item::Static(i) => named(EntryKind::NamedItem, &i.ident, &i.attrs, &i.vis),
        Item::Struct(i) => named(EntryKind::TypeItem, &i.ident, &i.attrs, &i.vis),
        Item::Trait(i) => named(EntryKind::TypeItem, &i.ident, &i.attrs, &i.vis),
        Item::TraitAlias(i) => named(EntryKind::TypeItem, &i.ident, &i.attrs, &i.vis),
        Item::Type(i) => named(EntryKind::TypeItem, &i.ident, &i.attrs, &i.vis),
        Item::Union(i) => named(EntryKind::TypeItem, &i.ident, &i.attrs, &i.vis),
        Item::Use(i) => {
            let mut leaves = UseLeaves::default();
            leaves.collect(&i.tree, None);
            let kind = EntryKind::Use {
                leaves: leaves.names,
                glob: leaves.glob,
            };
            Facts {
                public: is_public(&i.vis),
                ..unnamed(kind, &i.attrs)
            }
        }
        Item::ForeignMod(i) => unnamed(EntryKind::Other, &i.attrs),
        _ => unnamed(EntryKind::Other, &[]),
    }
}

/// Trait items carry no visibility of their own: they are public with the trait.
fn trait_item_facts(item: &TraitItem, trait_public: bool) -> Facts<'_> {
    let (name, attrs) = match item {
        TraitItem::Const(i) => (unraw(&i.ident), &i.attrs[..]),
        TraitItem::Fn(i) => (unraw(&i.sig.ident), &i.attrs[..]),
        TraitItem::Type(i) => (unraw(&i.ident), &i.attrs[..]),
        TraitItem::Macro(i) => (None, &i.attrs[..]),
        _ => (None, &[][..]),
    };
    Facts {
        kind: EntryKind::TraitItem,
        name,
        attrs,
        public: trait_public,
    }
}

fn impl_item_facts(item: &ImplItem, in_trait_impl: bool, self_ty: Option<String>) -> Facts<'_> {
    let (name, attrs, public) = match item {
        ImplItem::Const(i) => (unraw(&i.ident), &i.attrs[..], is_public(&i.vis)),
        ImplItem::Fn(i) => (unraw(&i.sig.ident), &i.attrs[..], is_public(&i.vis)),
        ImplItem::Type(i) => (unraw(&i.ident), &i.attrs[..], is_public(&i.vis)),
        ImplItem::Macro(i) => (None, &i.attrs[..], false),
        _ => (None, &[][..], false),
    };
    Facts {
        kind: EntryKind::ImplItem {
            in_trait_impl,
            self_ty,
        },
        name,
        attrs,
        public,
    }
}

/// The last path segment an impl's Self type names, through references,
/// parentheses and invisible groups; `None` for any other type form.
fn self_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(r) => self_type_name(&r.elem),
        Type::Paren(p) => self_type_name(&p.elem),
        Type::Group(g) => self_type_name(&g.elem),
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.unraw().to_string()),
        _ => None,
    }
}

/// The names a `use` tree brings into scope, and whether it globs.
#[derive(Default)]
struct UseLeaves {
    names: Vec<String>,
    glob: bool,
}

impl UseLeaves {
    fn collect(&mut self, tree: &UseTree, parent: Option<&Ident>) {
        match tree {
            UseTree::Path(path) => self.collect(&path.tree, Some(&path.ident)),
            UseTree::Name(name) if name.ident == "self" => {
                self.names.extend(parent.map(|p| p.unraw().to_string()))
            }
            UseTree::Name(name) => self.names.push(name.ident.unraw().to_string()),
            UseTree::Rename(rename) if rename.rename == "_" => {}
            UseTree::Rename(rename) => self.names.push(rename.rename.unraw().to_string()),
            UseTree::Glob(_) => self.glob = true,
            UseTree::Group(group) => {
                for item in &group.items {
                    self.collect(item, parent);
                }
            }
        }
    }
}

/// The pure classifier: evidence and shape violations from the whole inventory.
fn classify(entries: &[SurfaceEntry]) -> GatedSurface {
    let gated_types: BTreeSet<&str> = entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::TypeItem) && entry.gated())
        .filter_map(|entry| entry.name.as_deref())
        .collect();
    GatedSurface {
        evidence: entries
            .iter()
            .flat_map(|entry| evidence_names(entry, &gated_types))
            .collect(),
        violations: Vec::new(),
    }
}

/// The names a consumer must spell to reach a gated public position. Members
/// of a gated type add none: they are reachable only through the type's name.
fn evidence_names(entry: &SurfaceEntry, gated_types: &BTreeSet<&str>) -> Vec<String> {
    let name = || entry.name.iter().cloned().collect();
    if !entry.gated() {
        return Vec::new();
    }
    match &entry.kind {
        EntryKind::MacroRules => name(),
        EntryKind::Use { leaves, .. } if entry.public => leaves.clone(),
        EntryKind::Field | EntryKind::Variant | EntryKind::TraitItem if entry.inherited_gate => {
            Vec::new()
        }
        EntryKind::ImplItem {
            self_ty: Some(self_ty),
            ..
        } if gated_types.contains(self_ty.as_str()) => Vec::new(),
        _ if entry.public => name(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::fixture::Fixture;

    fn crate_with(files: &[(&str, &str)]) -> Fixture {
        let fx = Fixture::new();
        for (rel, contents) in files {
            fx.write(rel, contents);
        }
        fx
    }

    fn lib_root(fx: &Fixture) -> PathBuf {
        fx.root().join("src/lib.rs")
    }

    fn evidence(files: &[(&str, &str)]) -> BTreeSet<String> {
        let fx = crate_with(files);
        GatedSurface::scan(&lib_root(&fx), "f")
            .expect("fixture crate scans")
            .evidence
    }

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    const TRAP_CRATE: &[(&str, &str)] = &[
        (
            "src/lib.rs",
            "#[cfg(any(test, feature = \"f\"))]\npub mod probe;\npub mod graph;\npub mod invariants;\n",
        ),
        ("src/probe.rs", "pub fn inner() {}\n"),
        ("src/graph.rs", "pub fn build() {}\n"),
        ("src/invariants.rs", "pub fn check() {}\n"),
    ];

    #[test]
    fn module_gate_covers_only_the_module_it_is_attached_to() {
        let evidence = evidence(TRAP_CRATE);
        assert!(evidence.contains("probe"), "{evidence:?}");
        for ungated in ["graph", "invariants", "build", "check"] {
            assert!(
                !evidence.contains(ungated),
                "{ungated} leaked: {evidence:?}"
            );
        }
    }

    #[test]
    fn items_inside_a_gated_module_inherit_its_gate() {
        assert_eq!(evidence(TRAP_CRATE), names(&["probe", "inner"]));
    }

    #[test]
    fn gated_method_of_an_ungated_inherent_impl_is_evidence() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "pub struct Engine;\n\
             impl Engine {\n\
                 #[cfg(feature = \"f\")]\n\
                 pub fn hook(&self) {}\n\
                 pub fn plain(&self) {}\n\
             }\n",
        )]);
        assert_eq!(evidence, names(&["hook"]));
    }

    #[test]
    fn only_a_positive_mention_of_the_feature_gates() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(not(feature = \"f\"))]\npub fn off() {}\n\
             #[cfg(feature = \"other\")]\npub fn other() {}\n\
             #[cfg(not(any(unix, feature = \"f\")))]\npub fn nested_off() {}\n\
             #[cfg(all(unix, feature = \"f\"))]\npub fn both() {}\n\
             #[cfg(any(test, feature = \"f\"))]\npub fn either() {}\n\
             #[cfg(not(not(feature = \"f\")))]\npub fn double_negated() {}\n\
             #[cfg(feature = \"f\")]\npub fn direct() {}\n",
        )]);
        assert_eq!(
            evidence,
            names(&["both", "either", "double_negated", "direct"])
        );
    }

    #[test]
    fn only_public_gated_positions_are_evidence() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(feature = \"f\")]\nfn hidden() {}\n\
             #[cfg(feature = \"f\")]\npub(crate) fn crate_only() {}\n\
             pub struct Holder {\n\
                 #[cfg(feature = \"f\")]\n    pub slot: u8,\n\
                 #[cfg(feature = \"f\")]\n    hidden_slot: u8,\n\
                 pub plain: u8,\n\
             }\n\
             pub enum Mode {\n\
                 #[cfg(feature = \"f\")]\n    Probe,\n\
                 Normal,\n\
             }\n",
        )]);
        assert_eq!(evidence, names(&["slot", "Probe"]));
    }

    #[test]
    fn members_of_a_gated_type_add_no_evidence_beyond_the_type_name() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(feature = \"f\")]\nimpl Mock {\n    pub fn new() -> Self {\n        Mock { source: 0 }\n    }\n}\n\
             #[cfg(feature = \"f\")]\npub struct Mock {\n    pub source: u8,\n}\n\
             #[cfg(feature = \"f\")]\npub enum Kind {\n    Alpha,\n}\n\
             #[cfg(feature = \"f\")]\npub trait Hooked {\n    fn hooked(&self);\n}\n",
        )]);
        assert_eq!(evidence, names(&["Mock", "Kind", "Hooked"]));
    }

    #[test]
    fn impl_items_inherit_a_module_gate_when_self_is_ungated() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "pub struct Engine;\n\
             #[cfg(feature = \"f\")]\nmod hooks {\n    impl super::Engine {\n        pub fn hook(&self) {}\n    }\n}\n",
        )]);
        assert_eq!(evidence, names(&["hook"]));
    }

    #[test]
    fn gated_reexport_contributes_its_leaf_names_rename_aware() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "mod inner {\n    pub struct A;\n    pub struct B;\n}\n\
             #[cfg(feature = \"f\")]\npub use inner::{A, B as C};\n",
        )]);
        assert_eq!(evidence, names(&["A", "C"]));
    }

    #[test]
    fn gated_macro_rules_is_evidence() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(feature = \"f\")]\nmacro_rules! m {\n    () => {};\n}\n",
        )]);
        assert_eq!(evidence, names(&["m"]));
    }

    #[test]
    fn out_of_line_modules_resolve_in_both_layouts_and_under_inline_modules() {
        let gated = |name: &str| format!("#[cfg(feature = \"f\")]\npub fn {name}() {{}}\n");
        let evidence = evidence(&[
            (
                "src/lib.rs",
                "pub mod a;\npub mod c;\npub mod x {\n    pub mod y;\n}\n",
            ),
            ("src/a.rs", "pub mod b;\npub mod z {\n    pub mod w;\n}\n"),
            ("src/a/b.rs", &gated("in_b")),
            ("src/a/z/w.rs", &gated("in_w")),
            ("src/c/mod.rs", "pub mod d;\n"),
            ("src/c/d.rs", &gated("in_d")),
            ("src/x/y.rs", &gated("in_y")),
        ]);
        assert_eq!(evidence, names(&["in_b", "in_w", "in_d", "in_y"]));
    }

    #[test]
    fn path_attribute_resolves_relative_to_the_declaring_file() {
        let evidence = evidence(&[
            ("src/lib.rs", "pub mod a;\n"),
            (
                "src/a.rs",
                "#[path = \"elsewhere/impl_a.rs\"]\npub mod renamed;\n",
            ),
            (
                "src/elsewhere/impl_a.rs",
                "#[cfg(feature = \"f\")]\npub fn via_path() {}\n",
            ),
        ]);
        assert_eq!(evidence, names(&["via_path"]));
    }

    #[test]
    fn path_attribute_inside_an_inline_module_is_refused() {
        let fx = crate_with(&[
            (
                "src/lib.rs",
                "mod outer {\n    #[path = \"p.rs\"]\n    mod p;\n}\n",
            ),
            ("src/p.rs", ""),
        ]);
        let err = GatedSurface::scan(&lib_root(&fx), "f").unwrap_err();
        let lib = lib_root(&fx);
        assert!(
            matches!(&err, ScanError::UnsupportedPath { declared_in, line: 2 } if *declared_in == lib),
            "{err:?}"
        );
    }

    #[test]
    fn unresolved_module_is_an_error_naming_its_declaring_file() {
        let fx = crate_with(&[("src/lib.rs", "pub fn ok() {}\nmod missing;\n")]);
        let err = GatedSurface::scan(&lib_root(&fx), "f").unwrap_err();
        let lib = lib_root(&fx);
        assert!(
            matches!(
                &err,
                ScanError::UnresolvedModule { declared_in, line: 2, module }
                    if *declared_in == lib && module == "missing"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn unparsable_module_file_is_an_error_naming_that_file() {
        let fx = crate_with(&[
            ("src/lib.rs", "mod broken;\n"),
            ("src/broken.rs", "pub fn (\n"),
        ]);
        let err = GatedSurface::scan(&lib_root(&fx), "f").unwrap_err();
        let broken = fx.root().join("src/broken.rs");
        assert!(
            matches!(&err, ScanError::Parse { path, .. } if *path == broken),
            "{err:?}"
        );
    }

    fn violations(files: &[(&str, &str)]) -> (Fixture, Vec<ShapeViolation>) {
        let fx = crate_with(files);
        let violations = GatedSurface::scan(&lib_root(&fx), "f")
            .expect("fixture crate scans")
            .violations;
        (fx, violations)
    }

    fn at(fx: &Fixture, rel: &str, line: usize, shape: Shape) -> ShapeViolation {
        ShapeViolation {
            file: fx.root().join(rel),
            line,
            shape,
        }
    }

    #[test]
    fn cfg_attr_keyed_on_the_feature_is_a_violation() {
        let (fx, found) = violations(&[(
            "src/lib.rs",
            "#[cfg_attr(feature = \"f\", derive(Debug))]\npub struct S;\n\
             pub struct T {\n    #[cfg_attr(any(test, feature = \"f\"), allow(dead_code))]\n    pub x: u8,\n}\n\
             #[cfg_attr(feature = \"other\", derive(Debug))]\npub struct U;\n\
             #[cfg_attr(not(feature = \"f\"), derive(Debug))]\npub struct V;\n",
        )]);
        assert_eq!(
            found,
            vec![
                at(&fx, "src/lib.rs", 1, Shape::CfgAttr),
                at(&fx, "src/lib.rs", 4, Shape::CfgAttr),
            ]
        );
    }

    #[test]
    fn gated_trait_impl_for_a_gated_type_is_admissible_in_either_order() {
        let (_fx, found) = violations(&[(
            "src/lib.rs",
            "#[cfg(feature = \"f\")]\nimpl Default for Mock {\n    fn default() -> Self {\n        Mock\n    }\n}\n\
             #[cfg(feature = \"f\")]\npub struct Mock;\n",
        )]);
        assert_eq!(found, vec![]);
    }

    #[test]
    fn gated_trait_impl_for_an_ungated_type_is_a_violation() {
        let (fx, found) = violations(&[(
            "src/lib.rs",
            "pub struct Engine;\n\
             #[cfg(feature = \"f\")]\nimpl From<u8> for Engine {\n    fn from(_: u8) -> Self {\n        Engine\n    }\n}\n",
        )]);
        let shape = Shape::TraitImplForUngatedSelf {
            self_ty: "Engine".to_string(),
        };
        assert_eq!(found, vec![at(&fx, "src/lib.rs", 2, shape)]);
    }

    #[test]
    fn trait_impl_inheriting_a_module_gate_for_an_ungated_type_is_a_violation() {
        let (fx, found) = violations(&[
            (
                "src/lib.rs",
                "pub struct Engine;\n#[cfg(feature = \"f\")]\nmod hooks;\n",
            ),
            (
                "src/hooks.rs",
                "impl Clone for crate::Engine {\n    fn clone(&self) -> Self {\n        crate::Engine\n    }\n}\n",
            ),
        ]);
        let shape = Shape::TraitImplForUngatedSelf {
            self_ty: "Engine".to_string(),
        };
        assert_eq!(found, vec![at(&fx, "src/hooks.rs", 1, shape)]);
    }

    #[test]
    fn gated_item_inside_an_ungated_trait_impl_is_a_violation() {
        let (fx, found) = violations(&[(
            "src/lib.rs",
            "pub trait Kernel {\n    fn morph(&self) {}\n}\npub struct Gmsh;\n\
             impl Kernel for Gmsh {\n    #[cfg(feature = \"f\")]\n    fn morph(&self) {}\n}\n",
        )]);
        let shape = Shape::GatedItemInUngatedTraitImpl {
            item: "morph".to_string(),
        };
        assert_eq!(found, vec![at(&fx, "src/lib.rs", 6, shape)]);
    }

    #[test]
    fn gated_public_glob_reexport_is_a_violation() {
        let (fx, found) = violations(&[(
            "src/lib.rs",
            "mod inner {\n    pub fn a() {}\n}\n\
             #[cfg(feature = \"f\")]\npub use inner::*;\n\
             #[cfg(feature = \"f\")]\nuse inner::*;\n",
        )]);
        assert_eq!(found, vec![at(&fx, "src/lib.rs", 4, Shape::GlobReexport)]);
    }

    #[test]
    fn gated_item_position_macro_invocation_is_a_violation() {
        let (fx, found) = violations(&[
            (
                "src/lib.rs",
                "#[cfg(feature = \"f\")]\nsome_macro! { x }\n\
                 other_macro! { y }\n\
                 #[cfg(feature = \"f\")]\nmacro_rules! m {\n    () => {};\n}\n\
                 #[cfg(feature = \"f\")]\nmod registry;\n",
            ),
            ("src/registry.rs", "inventory::submit! { Thing }\n"),
        ]);
        assert_eq!(
            found,
            vec![
                at(&fx, "src/lib.rs", 1, Shape::ItemMacroInvocation),
                at(&fx, "src/registry.rs", 1, Shape::ItemMacroInvocation),
            ]
        );
    }

    #[test]
    fn gated_inherent_impl_on_an_ungated_type_is_admissible() {
        let fx = crate_with(&[(
            "src/lib.rs",
            "pub struct Engine;\n#[cfg(feature = \"f\")]\nimpl Engine {\n    pub fn hook(&self) {}\n}\n",
        )]);
        let surface = GatedSurface::scan(&lib_root(&fx), "f").expect("fixture crate scans");
        assert_eq!(surface.violations, vec![]);
        assert_eq!(surface.evidence, names(&["hook"]));
    }

    #[test]
    fn violations_across_files_are_reported_in_walk_order() {
        let (fx, found) = violations(&[
            ("src/lib.rs", "pub mod a;\npub mod b;\n"),
            (
                "src/a.rs",
                "#[cfg(feature = \"f\")]\npub use std::collections::*;\n",
            ),
            (
                "src/b.rs",
                "#[cfg_attr(feature = \"f\", derive(Debug))]\npub struct B;\n",
            ),
        ]);
        assert_eq!(
            found,
            vec![
                at(&fx, "src/a.rs", 1, Shape::GlobReexport),
                at(&fx, "src/b.rs", 1, Shape::CfgAttr),
            ]
        );
    }
}
