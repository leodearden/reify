//! The items a provider gates on one feature, read from its module tree.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::parse::ParseStream;
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
    AmbiguousSelfType { self_ty: String },
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CfgAttr => f.write_str("a cfg_attr keyed on the feature"),
            Self::TraitImplForUngatedSelf { self_ty } => {
                write!(f, "a trait impl for the ungated type `{self_ty}`")
            }
            Self::GatedItemInUngatedTraitImpl { item } => {
                write!(f, "the item `{item}` inside an ungated trait impl")
            }
            Self::GlobReexport => f.write_str("a glob re-export"),
            Self::ItemMacroInvocation => f.write_str("an item-position macro invocation"),
            Self::AmbiguousSelfType { self_ty } => write!(
                f,
                "an impl on `{self_ty}` (a name a gated and an ungated type share; rename one)"
            ),
        }
    }
}

#[derive(Debug)]
pub struct GatedSurface {
    /// Names a consumer spells only to reach gated surface.
    pub evidence: BTreeSet<String>,
    /// Gated names an ungated public position also carries: spelling one
    /// proves nothing about the feature.
    pub shared: BTreeSet<String>,
    pub violations: Vec<ShapeViolation>,
}

impl GatedSurface {
    pub fn scan(lib_root: &Path, feature: &str) -> Result<GatedSurface, ScanError> {
        let mut walker = Walker {
            feature,
            entries: Vec::new(),
        };
        let mod_dir = lib_root.parent().unwrap_or(Path::new("")).to_path_buf();
        let crate_root = Scope {
            gated: false,
            exported: true,
        };
        walker.walk_file(lib_root, mod_dir, crate_root)?;
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
    cfg_attr_gate: bool,
    public: bool,
    /// Public, and for a module-level item every enclosing module is too.
    exported: bool,
    file: PathBuf,
    line: usize,
}

impl SurfaceEntry {
    fn gated(&self) -> bool {
        self.own_gate || self.inherited_gate
    }
}

/// What an enclosing position passes down: its gate, and whether a public
/// position inside it is reachable from outside the crate.
#[derive(Clone, Copy)]
struct Scope {
    gated: bool,
    exported: bool,
}

impl Scope {
    /// Members (fields, variants, impl and trait items) are reached through
    /// their type, wherever the type is declared.
    fn members(self) -> Scope {
        Scope {
            exported: true,
            ..self
        }
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
    scope: Scope,
    inline: bool,
}

/// What the walk reads off one surface position before gating it.
struct Facts<'a> {
    kind: EntryKind,
    name: Option<String>,
    attrs: &'a [Attribute],
    public: bool,
}

impl<'a> Facts<'a> {
    fn unnamed(kind: EntryKind, attrs: &'a [Attribute]) -> Self {
        Facts {
            kind,
            name: None,
            attrs,
            public: false,
        }
    }
}

struct Walker<'f> {
    feature: &'f str,
    entries: Vec<SurfaceEntry>,
}

impl Walker<'_> {
    fn walk_file(&mut self, file: &Path, mod_dir: PathBuf, scope: Scope) -> Result<(), ScanError> {
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
            scope,
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
        let inner = self.record(facts, ctx.scope, &ctx.file, item);
        match item {
            Item::Mod(module) => self.walk_mod(module, ctx, inner)?,
            Item::Impl(imp) => {
                let self_ty = self_type_name(&imp.self_ty);
                for member in &imp.items {
                    let facts = impl_item_facts(member, imp.trait_.is_some(), self_ty.clone());
                    self.record(facts, inner.members(), &ctx.file, member);
                }
            }
            Item::Trait(tr) => {
                for member in &tr.items {
                    let facts = trait_item_facts(member, public);
                    self.record(facts, inner.members(), &ctx.file, member);
                }
            }
            Item::Struct(st) => self.walk_fields(&st.fields, inner.members(), &ctx.file),
            Item::Union(un) => self.walk_fields(&un.fields.named, inner.members(), &ctx.file),
            Item::Enum(en) => {
                for variant in &en.variants {
                    let facts = Facts {
                        kind: EntryKind::Variant,
                        name: Some(variant.ident.unraw().to_string()),
                        attrs: &variant.attrs,
                        public,
                    };
                    self.record(facts, inner.members(), &ctx.file, variant);
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
        scope: Scope,
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
                scope,
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
        self.walk_file(&file, mod_dir, scope)
    }

    fn walk_fields<'a>(
        &mut self,
        fields: impl IntoIterator<Item = &'a Field>,
        scope: Scope,
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
                self.record(facts, scope, file, field);
            }
        }
    }

    /// Gates one position (own cfg OR the enclosing gate), records it, and
    /// returns the scope it passes to the positions nested inside it.
    fn record(&mut self, facts: Facts, scope: Scope, file: &Path, node: &impl Spanned) -> Scope {
        let entry = SurfaceEntry {
            kind: facts.kind,
            name: facts.name,
            own_gate: self.gates(facts.attrs),
            inherited_gate: scope.gated,
            cfg_attr_gate: self.cfg_attr_gates(facts.attrs),
            public: facts.public,
            exported: facts.public && scope.exported,
            file: file.to_path_buf(),
            line: line_of(node),
        };
        let inner = Scope {
            gated: entry.gated(),
            exported: entry.exported,
        };
        self.entries.push(entry);
        inner
    }

    fn gates(&self, attrs: &[Attribute]) -> bool {
        attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg"))
            .filter_map(|attr| attr.parse_args::<Meta>().ok())
            .any(|predicate| positively_mentions(&predicate, self.feature))
    }

    fn cfg_attr_gates(&self, attrs: &[Attribute]) -> bool {
        attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg_attr"))
            .filter_map(|attr| attr.parse_args_with(cfg_attr_predicate).ok())
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

/// The predicate of `cfg_attr(predicate, attrs...)`; the attributes stay unparsed.
fn cfg_attr_predicate(input: ParseStream) -> syn::Result<Meta> {
    let predicate = input.parse()?;
    input.parse::<proc_macro2::TokenStream>()?;
    Ok(predicate)
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
            Facts::unnamed(kind, &i.attrs)
        }
        Item::Macro(i) => match &i.ident {
            Some(ident) => Facts {
                kind: EntryKind::MacroRules,
                name: unraw(ident),
                attrs: &i.attrs,
                public: false,
            },
            None => Facts::unnamed(EntryKind::MacroInvocation, &i.attrs),
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
                ..Facts::unnamed(kind, &i.attrs)
            }
        }
        Item::ForeignMod(i) => Facts::unnamed(EntryKind::Other, &i.attrs),
        _ => Facts::unnamed(EntryKind::Other, &[]),
    }
}

/// Trait items carry no visibility of their own: they are public with the trait.
fn trait_item_facts(item: &TraitItem, trait_public: bool) -> Facts<'_> {
    let (ident, attrs) = match item {
        TraitItem::Const(i) => (&i.ident, &i.attrs),
        TraitItem::Fn(i) => (&i.sig.ident, &i.attrs),
        TraitItem::Type(i) => (&i.ident, &i.attrs),
        TraitItem::Macro(i) => return Facts::unnamed(EntryKind::MacroInvocation, &i.attrs),
        _ => return Facts::unnamed(EntryKind::Other, &[]),
    };
    Facts {
        kind: EntryKind::TraitItem,
        name: unraw(ident),
        attrs,
        public: trait_public,
    }
}

fn impl_item_facts(item: &ImplItem, in_trait_impl: bool, self_ty: Option<String>) -> Facts<'_> {
    let (ident, attrs, vis) = match item {
        ImplItem::Const(i) => (&i.ident, &i.attrs, &i.vis),
        ImplItem::Fn(i) => (&i.sig.ident, &i.attrs, &i.vis),
        ImplItem::Type(i) => (&i.ident, &i.attrs, &i.vis),
        ImplItem::Macro(i) => return Facts::unnamed(EntryKind::MacroInvocation, &i.attrs),
        _ => return Facts::unnamed(EntryKind::Other, &[]),
    };
    Facts {
        kind: EntryKind::ImplItem {
            in_trait_impl,
            self_ty,
        },
        name: unraw(ident),
        attrs,
        public: is_public(vis),
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
    let types = &TypeNames::of(entries);
    let reach = Reach::of(entries);
    let ungated_names: BTreeSet<String> = entries
        .iter()
        .filter(|entry| !entry.gated())
        .flat_map(|entry| reach.spelled_names(entry))
        .collect();
    let (shared, evidence) = entries
        .iter()
        .filter(|entry| entry.gated() && !reached_through_gated_type(entry, types))
        .flat_map(|entry| reach.spelled_names(entry))
        .collect::<BTreeSet<String>>()
        .into_iter()
        .partition(|name| ungated_names.contains(name));
    GatedSurface {
        evidence,
        shared,
        violations: entries
            .iter()
            .flat_map(|entry| {
                SHAPE_RULES
                    .iter()
                    .filter_map(move |rule| rule(entry, types))
                    .map(move |shape| ShapeViolation {
                        file: entry.file.clone(),
                        line: entry.line,
                        shape,
                    })
            })
            .collect(),
    }
}

/// The provider's type names by gate. Identity is the bare name, so a name
/// declared both gated and ungated cannot say which type an impl is on.
struct TypeNames<'a> {
    gated: BTreeSet<&'a str>,
    ungated: BTreeSet<&'a str>,
}

impl<'a> TypeNames<'a> {
    fn of(entries: &'a [SurfaceEntry]) -> Self {
        let (gated, ungated) = entries
            .iter()
            .filter(|entry| matches!(entry.kind, EntryKind::TypeItem))
            .partition::<Vec<_>, _>(|entry| entry.gated());
        let names = |types: Vec<&'a SurfaceEntry>| {
            types
                .into_iter()
                .filter_map(|entry| entry.name.as_deref())
                .collect()
        };
        TypeNames {
            gated: names(gated),
            ungated: names(ungated),
        }
    }

    fn is_gated(&self, name: &str) -> bool {
        self.gated.contains(name)
    }

    fn is_ambiguous(&self, name: &str) -> bool {
        self.gated.contains(name) && self.ungated.contains(name)
    }
}

/// Which public positions a consumer can spell. A public item of a private
/// module is reachable only through a re-export, unless some ungated glob
/// re-export may expose any module.
struct Reach {
    any_module: bool,
}

impl Reach {
    fn of(entries: &[SurfaceEntry]) -> Self {
        let ungated_glob = |entry: &SurfaceEntry| {
            matches!(entry.kind, EntryKind::Use { glob: true, .. })
                && entry.public
                && !entry.gated()
        };
        Reach {
            any_module: entries.iter().any(ungated_glob),
        }
    }

    /// The names a consumer spells to reach a position, gated or not.
    fn spelled_names(&self, entry: &SurfaceEntry) -> Vec<String> {
        let reachable = if self.any_module {
            entry.public
        } else {
            entry.exported
        };
        match &entry.kind {
            EntryKind::MacroRules => entry.name.iter().cloned().collect(),
            EntryKind::Use { leaves, .. } if reachable => leaves.clone(),
            _ if reachable => entry.name.iter().cloned().collect(),
            _ => Vec::new(),
        }
    }
}

/// Members of a gated type add no evidence: they are reachable only through
/// the type's name.
fn reached_through_gated_type(entry: &SurfaceEntry, types: &TypeNames) -> bool {
    match &entry.kind {
        EntryKind::Field | EntryKind::Variant | EntryKind::TraitItem => entry.inherited_gate,
        EntryKind::ImplItem {
            self_ty: Some(self_ty),
            ..
        } => types.is_gated(self_ty),
        _ => false,
    }
}

/// One admissibility rule: the shape by which a position exposes gated
/// surface that a consumer can reach without naming an evidence item.
type ShapeRule = fn(&SurfaceEntry, &TypeNames) -> Option<Shape>;

const SHAPE_RULES: [ShapeRule; 6] = [
    cfg_attr_shape,
    trait_impl_for_ungated_self,
    gated_item_in_ungated_trait_impl,
    glob_reexport,
    item_macro_invocation,
    ambiguous_self_type,
];

/// Reported as the Self of a trait impl whose Self type is not a path.
const NON_PATH_SELF_TYPE: &str = "<non-path type>";

fn cfg_attr_shape(entry: &SurfaceEntry, _: &TypeNames) -> Option<Shape> {
    entry.cfg_attr_gate.then_some(Shape::CfgAttr)
}

fn trait_impl_for_ungated_self(entry: &SurfaceEntry, types: &TypeNames) -> Option<Shape> {
    let EntryKind::Impl {
        trait_impl: true,
        self_ty,
    } = &entry.kind
    else {
        return None;
    };
    let self_is_gated = self_ty.as_deref().is_some_and(|ty| types.is_gated(ty));
    (entry.gated() && !self_is_gated).then(|| Shape::TraitImplForUngatedSelf {
        self_ty: self_ty.as_deref().unwrap_or(NON_PATH_SELF_TYPE).to_string(),
    })
}

fn gated_item_in_ungated_trait_impl(entry: &SurfaceEntry, _: &TypeNames) -> Option<Shape> {
    let EntryKind::ImplItem {
        in_trait_impl: true,
        ..
    } = &entry.kind
    else {
        return None;
    };
    let item = entry.name.clone()?;
    (entry.own_gate && !entry.inherited_gate).then_some(Shape::GatedItemInUngatedTraitImpl { item })
}

fn glob_reexport(entry: &SurfaceEntry, _: &TypeNames) -> Option<Shape> {
    let EntryKind::Use { glob: true, .. } = &entry.kind else {
        return None;
    };
    (entry.gated() && entry.public).then_some(Shape::GlobReexport)
}

fn item_macro_invocation(entry: &SurfaceEntry, _: &TypeNames) -> Option<Shape> {
    (matches!(entry.kind, EntryKind::MacroInvocation) && entry.gated())
        .then_some(Shape::ItemMacroInvocation)
}

/// A gated impl, or a gated item of an ungated inherent impl, is judged by
/// whether its Self is a gated type; an ambiguous Self name cannot say.
fn ambiguous_self_type(entry: &SurfaceEntry, types: &TypeNames) -> Option<Shape> {
    let self_ty = match &entry.kind {
        EntryKind::Impl { self_ty, .. } if entry.gated() => self_ty,
        EntryKind::ImplItem {
            in_trait_impl: false,
            self_ty,
        } if entry.own_gate && !entry.inherited_gate => self_ty,
        _ => return None,
    };
    let self_ty = self_ty.as_deref()?;
    types
        .is_ambiguous(self_ty)
        .then(|| Shape::AmbiguousSelfType {
            self_ty: self_ty.to_string(),
        })
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

    fn surface(lib: &str) -> GatedSurface {
        let fx = crate_with(&[("src/lib.rs", lib)]);
        GatedSurface::scan(&lib_root(&fx), "f").expect("fixture crate scans")
    }

    #[test]
    fn a_gated_name_a_reachable_ungated_item_also_carries_is_shared_not_evidence() {
        let found = surface(
            "pub mod realization_cache {\n    pub const NO_OPTIONS: u8 = 0;\n}\n\
             #[cfg(not(feature = \"f\"))]\npub fn twin() {}\n\
             #[cfg(feature = \"f\")]\npub fn twin() {}\n\
             fn hook() {}\n\
             pub struct Engine;\n\
             impl Engine {\n\
                 #[cfg(feature = \"f\")]\n    pub fn realization_cache(&self) {}\n\
                 #[cfg(feature = \"f\")]\n    pub fn hook(&self) {}\n\
             }\n",
        );
        assert_eq!(found.evidence, names(&["hook"]));
        assert_eq!(found.shared, names(&["realization_cache", "twin"]));
    }

    #[test]
    fn a_private_modules_public_item_is_reachable_only_through_a_reexport() {
        let lib = "mod inner {\n    pub fn hook() {}\n    #[cfg(feature = \"f\")]\n    pub fn hidden() {}\n}\n\
                   pub struct Engine;\n\
                   impl Engine {\n    #[cfg(feature = \"f\")]\n    pub fn hook(&self) {}\n}\n";
        let private = surface(lib);
        assert_eq!(
            (private.evidence, private.shared),
            (names(&["hook"]), names(&[]))
        );

        let globbed = surface(&format!("{lib}pub use inner::*;\n"));
        assert_eq!(
            (globbed.evidence, globbed.shared),
            (names(&["hidden"]), names(&["hook"])),
            "an ungated glob re-export may expose any module"
        );
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
    fn a_gated_trait_impl_on_a_name_gated_and_ungated_types_share_is_a_violation() {
        let (fx, found) = violations(&[(
            "src/lib.rs",
            "pub mod other {\n    pub struct Mock;\n}\n\
             #[cfg(feature = \"f\")]\npub struct Mock;\n\
             #[cfg(feature = \"f\")]\nimpl Clone for other::Mock {\n    fn clone(&self) -> Self {\n        other::Mock\n    }\n}\n",
        )]);
        let shape = Shape::AmbiguousSelfType {
            self_ty: "Mock".to_string(),
        };
        assert_eq!(found, vec![at(&fx, "src/lib.rs", 6, shape)]);
    }

    #[test]
    fn gated_inherent_surface_on_a_name_gated_and_ungated_types_share_is_a_violation() {
        let (fx, found) = violations(&[(
            "src/lib.rs",
            "pub mod other {\n    pub struct Mock;\n    impl Mock {\n        \
             #[cfg(feature = \"f\")]\n        pub fn hook(&self) {}\n        \
             pub fn plain(&self) {}\n    }\n    \
             #[cfg(feature = \"f\")]\n    impl Mock {\n        pub fn gated_block(&self) {}\n    }\n}\n\
             #[cfg(feature = \"f\")]\npub struct Mock;\n",
        )]);
        let shape = || Shape::AmbiguousSelfType {
            self_ty: "Mock".to_string(),
        };
        assert_eq!(
            found,
            vec![
                at(&fx, "src/lib.rs", 4, shape()),
                at(&fx, "src/lib.rs", 8, shape()),
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
