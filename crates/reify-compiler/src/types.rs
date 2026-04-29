use std::collections::{BTreeMap, HashMap, HashSet};

use reify_types::{
    CompiledExpr, ConstraintDomain, ConstraintNodeId, ContentHash, DimensionVector,
    OptimizationObjective, RealizationNodeId, SourceSpan, Type, ValueCellId,
};

pub use reify_types::{CompiledFnBody, CompiledFunction};

/// A compiled import declaration.
#[derive(Debug, Clone)]
pub struct CompiledImport {
    pub path: String,
    pub kind: reify_syntax::ImportKind,
    pub is_pub: bool,
    pub span: SourceSpan,
}

/// A compiled trait definition.
#[derive(Debug, Clone)]
pub struct CompiledTrait {
    pub name: String,
    pub is_pub: bool,
    /// Type parameters declared on this trait (e.g., `<T: Rigid>`).
    pub type_params: Vec<reify_types::TypeParam>,
    /// Names of traits this trait refines (parent traits).
    pub refinements: Vec<String>,
    /// Members that conforming structures must provide (no default).
    pub required_members: Vec<TraitRequirement>,
    /// Members with defaults that are injected if the structure doesn't override.
    pub defaults: Vec<TraitDefault>,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
    /// Block-level pragmas from the parsed declaration (e.g., `#precision(bits=32)`).
    pub pragmas: Vec<reify_syntax::Pragma>,
}

/// A required member in a trait — conforming structures must provide this.
#[derive(Debug, Clone)]
pub struct TraitRequirement {
    pub name: String,
    pub kind: RequirementKind,
    pub span: SourceSpan,
}

/// The kind of requirement a trait imposes.
#[derive(Debug, Clone)]
pub enum RequirementKind {
    /// A param with a specific type: `param x : Length`
    Param(Type),
    /// A let with a specific type: `let x : Length`
    Let(Type),
    /// A sub-component: `sub hole = Hole`
    Sub(String),
}

/// A default member provided by a trait — injected if not overridden.
#[derive(Debug, Clone)]
pub struct TraitDefault {
    pub name: Option<String>,
    pub kind: DefaultKind,
    pub span: SourceSpan,
}

/// The kind of default a trait provides.
///
/// Exhaustive by design: adding a variant here must fail compilation at every
/// match site (reify-compiler internals, reify-test-support builders, and the
/// silent-defaults integration tests) so the new variant cannot silently slip
/// through conformance, content-hash, or regression paths.
#[derive(Debug, Clone)]
pub enum DefaultKind {
    /// A param with a default expression: `param x : Length = 10mm`
    Param {
        cell_type: Type,
        default_decl: reify_syntax::ParamDecl,
    },
    /// A let with a value expression: `let x = expr`
    Let {
        /// The resolved type from the annotation (e.g. `let x : Length = …` → `Some(Type::length())`).
        /// `None` when no annotation is present.
        cell_type: Option<reify_types::Type>,
        let_decl: reify_syntax::LetDecl,
    },
    /// A constraint with an expression: `constraint label : expr`
    Constraint(reify_syntax::ConstraintDecl),
}

/// The compiled source of a field.
#[derive(Debug, Clone)]
pub enum CompiledFieldSource {
    /// Analytical field: defined by a lambda expression.
    Analytical { expr: CompiledExpr },
    /// Sampled field: defined by config key-value pairs.
    Sampled { config: Vec<(String, CompiledExpr)> },
    /// Composed field: defined by a composition lambda.
    Composed { expr: CompiledExpr },
    /// Imported field: placeholder for externally-sourced field data.
    Imported,
}

/// A compiled field declaration.
#[derive(Debug, Clone)]
pub struct CompiledField {
    pub name: String,
    pub is_pub: bool,
    pub domain_type: Type,
    pub codomain_type: Type,
    pub source: CompiledFieldSource,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
}

/// A compiled purpose parameter — binds an entity reference.
#[derive(Debug, Clone)]
pub struct CompiledPurposeParam {
    pub name: String,
    pub entity_kind: String,
}

/// A resolved reflective schema query — e.g., `subject.params` resolved to concrete ValueCellIds.
#[derive(Debug, Clone)]
pub struct ResolvedSchemaQuery {
    /// The purpose parameter name this query was on (e.g., "subject").
    pub param_name: String,
    /// The kind of schema query (e.g., "params", "geometric_params", "ports").
    pub query_kind: String,
    /// The resolved ValueCellIds from the bound entity's TopologyTemplate.
    pub resolved_ids: Vec<ValueCellId>,
}

/// A compiled purpose declaration.
#[derive(Debug, Clone)]
pub struct CompiledPurpose {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<CompiledPurposeParam>,
    pub constraints: Vec<CompiledConstraint>,
    pub objective: Option<OptimizationObjective>,
    /// Reflective schema queries resolved at compile time.
    pub resolved_queries: Vec<ResolvedSchemaQuery>,
    pub content_hash: ContentHash,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
    /// Block-level pragmas from the parsed declaration (e.g., `#solver(method="gradient")`).
    pub pragmas: Vec<reify_syntax::Pragma>,
}

/// A compiled module — the output of the compiler.
#[derive(Debug, Clone)]
pub struct CompiledModule {
    pub path: reify_types::ModulePath,
    pub imports: Vec<CompiledImport>,
    pub enum_defs: Vec<reify_types::EnumDef>,
    pub functions: Vec<CompiledFunction>,
    pub trait_defs: Vec<CompiledTrait>,
    pub fields: Vec<CompiledField>,
    pub compiled_purposes: Vec<CompiledPurpose>,
    pub templates: Vec<TopologyTemplate>,
    /// Compiled unit declarations from this module.
    pub units: Vec<CompiledUnit>,
    /// Compiled type alias declarations from this module.
    pub type_aliases: Vec<CompiledTypeAlias>,
    /// Constraint definitions declared in this module — both pub and non-pub.
    ///
    /// Only entries with `is_pub == true` are propagated into downstream modules
    /// via the prelude mechanism (see `compile_with_prelude_refs`); callers that
    /// want the exported subset must apply the `is_pub` filter themselves.
    pub constraint_defs: Vec<CompiledConstraintDef>,
    /// Module-level pragmas declared in this module (e.g., `#no_prelude`, `#precision`).
    /// All pragmas are stored here, including consumed ones like `#no_prelude`.
    pub pragmas: Vec<reify_syntax::Pragma>,
    /// Module-level `#precision` value in metres, or None when absent / when the
    /// pragma was malformed.
    ///
    /// Populated by `module_pragmas::apply_module_pragmas` from the first
    /// well-formed `#precision(<Length-quantity>)` module-level pragma. Consumed
    /// downstream by `Engine::effective_tessellation_tolerance` to override the
    /// default OCCT tessellation tolerance.
    pub default_tolerance: Option<f64>,
    /// Module-level `#version` value as a (MAJOR, MINOR) pair, or None when
    /// absent / when the pragma was malformed / when there were duplicates.
    ///
    /// Populated by `module_pragmas::apply_module_pragmas` from the first
    /// well-formed `#version(...)` module-level pragma. Storage reflects what
    /// the user declared regardless of the validation outcome (too-new error,
    /// too-old warning, or in-range silent), so downstream tooling (e.g. doc
    /// generators) can render the user's intent verbatim.
    pub declared_version: Option<(u16, u16)>,
    /// Module-level `#solver` back-end name + options, or None when absent /
    /// when the pragma was malformed.
    ///
    /// Populated by `module_pragmas::apply_module_pragmas` from the first
    /// well-formed `#solver(<back-end-ident>, [key=value, ...])` module-level
    /// pragma. Storage reflects the user-declared name regardless of whether
    /// the back-end appears in the v0.1 known-name list, mirroring `#version`'s
    /// storage-reflects-declared policy: downstream consumers (doc generator,
    /// runtime registry lookup) need the verbatim name, and tying storage to
    /// validation outcome would force them to re-derive it from
    /// `module.pragmas`.
    pub solver_pragma: Option<SolverPragma>,
    /// Module-level `#kernel` value (the user-declared kernel ident), or None
    /// when absent / when the pragma was malformed.
    ///
    /// Populated by `module_pragmas::apply_module_pragmas` from the first
    /// well-formed `#kernel(<ident>)` module-level pragma. Storage reflects
    /// the user-declared name regardless of validation outcome (per PRD §4 —
    /// round-trip + doc-tool consumption), mirroring the policy used by
    /// `declared_version` and `solver_pragma`. v0.1 dispatch always uses OCCT
    /// regardless of the stored value: a non-`occt` ident produces an
    /// error-level diagnostic, but the user-declared name is still stored so
    /// downstream tooling (doc generator, future kernel-registry lookup) sees
    /// the verbatim intent. Only malformed shapes (zero args, key=value-first,
    /// non-Ident bare values) leave the field as None.
    pub kernel_pragma: Option<String>,
    pub diagnostics: Vec<reify_types::Diagnostic>,
    pub content_hash: ContentHash,
}

/// Module-level `#solver(<back-end-ident>, [key=value, ...])` value extracted
/// from the first well-formed `#solver` module pragma by
/// `module_pragmas::apply_module_pragmas`.
///
/// `name` is the back-end identifier (e.g. `"libslvs"`, `"argmin"`); `options`
/// holds the optional `key=value` arguments in alphabetical key order. The
/// `BTreeMap` choice (rather than `HashMap` or `Vec<(String, PragmaValue)>`)
/// matches the PRD specification at `docs/prds/pragmas.md` §3 and provides
/// deterministic iteration for downstream rendering / hashing consumers.
#[derive(Debug, Clone)]
pub struct SolverPragma {
    /// User-declared back-end identifier (verbatim).
    pub name: String,
    /// Optional `key = value` arguments, in alphabetical key order.
    pub options: BTreeMap<String, reify_syntax::PragmaValue>,
}

impl CompiledModule {
    /// Returns all templates tagged with `@test`.
    ///
    /// This is the canonical filter for test entities — consumers should prefer
    /// this over scanning `template.annotations` manually. Per Task 267, test
    /// entities are excluded from the normal evaluation graph.
    pub fn test_templates(&self) -> impl Iterator<Item = &TopologyTemplate> {
        self.templates.iter().filter(|t| t.is_test())
    }

    /// Returns all templates NOT tagged with `@test`.
    ///
    /// These are the templates that participate in the normal evaluation graph.
    pub fn non_test_templates(&self) -> impl Iterator<Item = &TopologyTemplate> {
        self.templates.iter().filter(|t| !t.is_test())
    }

    /// Returns all constraint defs tagged with `@test`.
    ///
    /// Uses `CompiledConstraintDef::is_test()` as the canonical predicate, which
    /// delegates to `reify_types::annotation::has_test_annotation` on the already-lowered
    /// annotation vec.
    pub fn test_constraint_defs(&self) -> impl Iterator<Item = &CompiledConstraintDef> {
        self.constraint_defs.iter().filter(|d| d.is_test())
    }

    /// Returns all constraint defs NOT tagged with `@test`.
    pub fn non_test_constraint_defs(&self) -> impl Iterator<Item = &CompiledConstraintDef> {
        self.constraint_defs.iter().filter(|d| !d.is_test())
    }

    /// Returns all functions tagged with `@test`.
    ///
    /// Uses `CompiledFunction::is_test()` as the canonical predicate.
    pub fn test_functions(&self) -> impl Iterator<Item = &CompiledFunction> {
        self.functions.iter().filter(|f| f.is_test())
    }

    /// Returns all functions NOT tagged with `@test`.
    pub fn non_test_functions(&self) -> impl Iterator<Item = &CompiledFunction> {
        self.functions.iter().filter(|f| !f.is_test())
    }
}

/// Whether a TopologyTemplate was compiled from a structure or an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Structure,
    Occurrence,
}

impl EntityKind {
    /// Returns the canonical string label for this variant as a `&'static str`.
    ///
    /// This is the single source of truth for the `"structure"` / `"occurrence"`
    /// literals used across the compiler and GUI. The `Display` impl delegates
    /// here so `as_label()` and `to_string()` can never diverge.
    pub const fn as_label(&self) -> &'static str {
        match self {
            EntityKind::Structure => "structure",
            EntityKind::Occurrence => "occurrence",
        }
    }
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_label())
    }
}

/// A captured per-element body template for a statement-form `forall` over
/// a deferred-count collection sub (task 2629; PRD criterion 7 second-half).
///
/// Stored on `TopologyTemplate.forall_templates` at compile time when
/// `resolve_forall_elements` cannot statically resolve the collection sub's
/// count (count cell missing or non-literal). The runtime
/// `Engine::edit_param` collection-count phase walks these templates whenever
/// a count cell becomes known and emits per-element constraints/connections
/// by rewriting `coll_sub[0]` placeholder cell IDs to `coll_sub[i]`.
///
/// **Hash stability:** `TopologyTemplate.content_hash` does NOT include
/// `forall_templates` — captures here are an internal runtime detail that
/// must not change the cache key for existing compile-time-resolvable
/// foralls. See `TopologyTemplateBuilder::build` for the comment.
#[derive(Debug, Clone)]
pub struct CompiledForallTemplate {
    /// The `forall v in ...:` bound variable name (e.g. "v").
    pub variable: String,
    /// The owning structure entity name (e.g. "S"). Used as the parent for
    /// scoped child cell IDs at runtime: `format!("{parent}.{sub}[{i}]")`.
    pub parent_entity: String,
    /// The collection sub-component name (e.g. "vents") on which this
    /// `forall` iterates.
    pub collection_sub_name: String,
    /// The count cell ValueCellId (e.g. `S.__count_vents`) whose value
    /// determines the number of per-element emissions.
    pub count_cell: ValueCellId,
    /// Source span anchored at the original `forall` declaration; used for
    /// per-element diagnostic provenance at runtime.
    pub span: SourceSpan,
    /// The per-element body shape — Constraint or Connect.
    pub body: CompiledForallBody,
}

/// The body shape of a captured forall template (task 2629 + task 2690).
///
/// Captured at compile time, consumed at runtime by `Engine::edit_param`'s
/// collection-count phase when a `__count_<sub>` cell becomes known. The
/// `Constraint` arm was wired by task 2629; the `Connect` arm by task 2690.
/// The `Instantiation` / `Chain` source-level shapes retain compile-time
/// silent-skip semantics — see `forall_elaborate.rs` info diagnostics.
#[derive(Debug, Clone)]
pub enum CompiledForallBody {
    /// Per-element constraint body: `forall v in coll: constraint <expr>`.
    ///
    /// `body_expr` is the body constraint expression with `v` substituted to
    /// `coll[0]` and run through `compile_expr` once at compile time. At
    /// runtime, `map_value_refs` rewrites every cell ID whose entity equals
    /// `format!("{parent}.{sub}[0]")` to `format!("{parent}.{sub}[{i}]")` and
    /// the resulting expression becomes the per-element constraint's `expr`.
    ///
    /// **Where-clause-bearing bodies are NOT captured here** (task 2629 step-24,
    /// reviewer-flagged): the runtime engine has no guarded-group plumbing
    /// for per-element where clauses, so deferred-count forall bodies with
    /// a `where` clause are treated as future scope alongside
    /// `Instantiation` / `Chain`. See `forall_elaborate.rs`'s
    /// `Deferred / ForallConstraintBody::Constraint` arm — it emits an info
    /// diagnostic and returns early without pushing a template. Re-adding
    /// where-clause support here MUST also wire guarded-group emission in
    /// `engine_edit.rs`.
    Constraint {
        /// Compiled body expression with placeholder cells (entity == `<parent>.<sub>[0]`).
        ///
        /// **Semantic divergence vs. the resolved (non-deferred) path**
        /// (reviewer flag, esc-2629-25 #2): in the resolved path, a body
        /// containing an explicit `coll[0]` reference stays as `coll[0]` for
        /// every iteration. Here, the runtime rewriter in `engine_edit.rs`
        /// matches every cell whose entity equals `<parent>.<sub>[0]`, so a
        /// user-written `coll[0]` ref inside the body is *also* rewritten to
        /// `coll[i]` for each `i`. This is vanishingly rare in practice (the
        /// bound variable `v` is the only natural way to reference an element
        /// inside a `forall v in coll` body), but a real divergence — pin a
        /// test if a use case appears, or migrate to a sentinel placeholder
        /// entity that never collides with user-written index expressions.
        body_expr: CompiledExpr,
    },
    /// Per-element connection body: `forall v in coll: connect <l> <op> <r> [: T(...)]`.
    ///
    /// Task 2690 — wired. The substituted `coll[0]` placeholder appears
    /// pre-baked in `left_port_template` and `right_port_template` (e.g.
    /// `"vents[0].inlet"`); at runtime the engine rewrites each occurrence
    /// of `format!("{coll_sub_name}[0]")` to `format!("{coll_sub_name}[{i}]")`
    /// to materialise per-element connections.
    ///
    /// **Semantic divergence vs. the resolved (non-deferred) path** (mirrors
    /// the Constraint variant's `body_expr` doc): a user-written explicit
    /// `coll[0]` reference inside the connect body — vanishingly rare in
    /// practice — is *also* rewritten to `coll[i]` for each `i`. Anchoring
    /// the substring on `coll_sub_name` (not just `[0]`) reduces the risk of
    /// accidentally rewriting an unrelated literal `[0]` in a port name.
    ///
    /// At runtime emission, the engine synthesises a fresh
    /// compatibility-constraint `ConstraintNodeId` per element and pushes a
    /// fresh `CompiledConnection` into `EvaluationGraph::connections`. The
    /// synthetic compatibility constraint is a `Bool::True` literal — the
    /// connect-time direction-check is NOT replicated at runtime; this
    /// mirrors the Constraint arm's "compile substitution; trust at runtime"
    /// policy. Auto-creation of `connector_sub` / `frame_constraint` is
    /// deferred to a future task.
    ///
    /// **Connector-spec drop at runtime** (task 2690 amendment): for the
    /// rich form (`forall v in coll: connect a -> b : T(p = e, ...)`) the
    /// captured `connector_type` and `params` fields are populated here at
    /// compile time, but `engine_edit.rs`'s re-emission drops them — only
    /// the port-to-port connection is materialised, with `connector_sub`
    /// set to `None` and `port_mappings` carried verbatim. The deferred
    /// compile-time path emits an info diagnostic surfacing this limitation;
    /// the captured fields are kept for a future task that wires
    /// connector-spec-aware runtime emission.
    Connect {
        /// Substituted left-side port name (e.g. `"vents[0].inlet"`).
        left_port_template: String,
        /// Direction operator from the source `connect` declaration.
        operator: reify_syntax::ConnectOp,
        /// Substituted right-side port name (e.g. `"air_channel"`).
        right_port_template: String,
        /// Optional explicit connector type name (e.g. `BoltSet`).
        connector_type: Option<String>,
        /// Pre-substituted, pre-compiled connect parameters
        /// (e.g. `[("grade", CompiledExpr::Literal(Int(8)))]`).
        params: Vec<(String, CompiledExpr)>,
        /// Explicit port-mappings (left_member, right_member). Carried
        /// through verbatim from the source declaration.
        port_mappings: Vec<(String, String)>,
    },
}

/// A topology template — compiled from a StructureDef or OccurrenceDef.
/// Contains all the value cells, constraints, and realizations.
#[derive(Debug, Clone)]
pub struct TopologyTemplate {
    pub name: String,
    pub entity_kind: EntityKind,
    pub visibility: Visibility,
    /// Type parameters declared on this structure (e.g., `<T: Rigid>`).
    pub type_params: Vec<reify_types::TypeParam>,
    /// Names of traits this structure declares conformance to (e.g., `["Rigid"]`).
    pub trait_bounds: Vec<String>,
    pub value_cells: Vec<ValueCellDecl>,
    pub constraints: Vec<CompiledConstraint>,
    pub realizations: Vec<RealizationDecl>,
    pub sub_components: Vec<SubComponentDecl>,
    pub ports: Vec<CompiledPort>,
    pub connections: Vec<CompiledConnection>,
    pub guarded_groups: Vec<CompiledGuardedGroup>,
    /// ValueCellIds whose boolean value controls topology (guard cells).
    pub structure_controlling: HashSet<ValueCellId>,
    pub objective: Option<OptimizationObjective>,
    /// Key-value entries from the entity's `meta { ... }` block (if any).
    pub meta: HashMap<String, String>,
    pub content_hash: ContentHash,
    /// True if this template participates in a recursive sub-component cycle.
    ///
    /// **Producer** — set to `true` by `detect_recursive_structures` in `scc.rs` during
    /// the post-compilation Tarjan SCC pass. Every template that belongs to a non-trivial
    /// SCC (i.e., is reachable from itself via sub-component edges) is marked here.
    /// The flag is also mixed into `content_hash` (`compiler::lib.rs`) so that otherwise
    /// identical templates with different recursion topology produce distinct cache keys.
    ///
    /// **Consumers** (runtime) — the flag is used in the following places by code that
    /// runs *after* compilation:
    /// - `termination.rs`: gates the termination-condition check so non-recursive templates
    ///   are skipped early.
    /// - `reify-eval/src/unfold.rs` (`collect_recursive_subs`): gates child-sub collection
    ///   to prevent the evaluator from treating non-recursive subs as candidates for
    ///   `unfold_recursive_sub`.
    /// - `reify-eval/src/engine_eval.rs` (`elaborate_root_frame`): gates root-frame
    ///   elaboration and activates `unfold_recursive_sub` for the recursive path.
    /// - `gui/src-tauri/src/engine.rs` (`build_template_node`): gates Design Tree traversal
    ///   to prevent infinite descent into non-terminating sub-graphs.
    ///
    /// The field is intentionally named `is_recursive` (not `has_recursive_structure`) to
    /// avoid a cascading rename across ~15 call sites in compiler, eval, GUI, test-support
    /// builder, and test files. See task 205 review / task 424 for the rationale.
    pub is_recursive: bool,
    /// Compiled annotations carried over from the parsed declaration.
    pub annotations: Vec<reify_types::Annotation>,
    /// Block-level pragmas from the parsed declaration (e.g., `#solver(backend="ipopt")`).
    pub pragmas: Vec<reify_syntax::Pragma>,
    /// Match-arm decl clusters registered during compilation (task 2372, step-10).
    ///
    /// Populated by `compile_match_arm_decl_group` in `entity.rs`.  Empty for
    /// templates that contain no `MatchArmDeclGroup` members.
    ///
    /// Production consumers (union typing, eval) are wired in task 2373 when a
    /// downstream stage first needs the data.  The field is always present (not
    /// `#[cfg(test)]`) so integration tests in `crates/reify-compiler/tests/`
    /// can access it — integration tests compile the library *without* cfg(test).
    pub match_arm_groups: Vec<GuardedDeclGroup>,
    /// Captured per-element body templates for statement-form `forall` over
    /// deferred-count collection subs (task 2629; PRD criterion 7 second-half).
    ///
    /// Empty for templates with no `forall` statements over deferred-count
    /// collections. Populated by `forall_elaborate::elaborate_forall_constraint`
    /// / `elaborate_forall_connect` when the collection's count cannot be
    /// statically resolved at compile time.
    ///
    /// **Hash stability:** intentionally NOT mixed into `content_hash`; the
    /// `compile_structure_inner` hash and the `TopologyTemplateBuilder.build`
    /// hash both omit this field so cache keys are stable across the addition
    /// of this runtime-only metadata. Consumers that need a fingerprint that
    /// varies with these templates must hash them externally.
    pub forall_templates: Vec<CompiledForallTemplate>,
}

impl TopologyTemplate {
    /// Returns `true` if this template is tagged with the `@test` annotation.
    ///
    /// Derived from `annotations` on each call (linear scan) — symmetric with
    /// `ConstraintDef::is_test()`. Annotation lists are typically 0–2 items,
    /// so the per-call cost is negligible; if profiling ever flags this,
    /// a cached `bool` field can be reintroduced.
    pub fn is_test(&self) -> bool {
        reify_types::annotation::has_test_annotation(&self.annotations)
    }
}

/// Look up a topology template by name in a slice of compiled templates.
///
/// Returns `Some(&template)` for the first match, or `None` if no template has
/// the given name.  All callers keep their own error-handling (diagnostic
/// emission, silent skip, or test panic) — this utility is policy-neutral.
pub fn find_template<'a>(
    templates: &'a [TopologyTemplate],
    name: &str,
) -> Option<&'a TopologyTemplate> {
    templates.iter().find(|t| t.name == name)
}

/// A compiled connection between ports — compiled from a ConnectDecl or desugared from a ChainDecl.
#[derive(Debug, Clone)]
pub struct CompiledConnection {
    pub left_port: String,
    pub operator: reify_syntax::ConnectOp,
    pub right_port: String,
    pub connector_sub: Option<String>,
    pub compatibility_constraint: ConstraintNodeId,
    pub port_mappings: Vec<(String, String)>,
    pub frame_constraint: Option<ConstraintNodeId>,
    pub span: SourceSpan,
}

/// A compiled port declaration — compiled from a PortDecl.
#[derive(Debug, Clone)]
pub struct CompiledPort {
    pub name: String,
    pub direction: reify_types::PortDirection,
    pub type_name: String,
    pub members: Vec<ValueCellDecl>,
    pub constraints: Vec<CompiledConstraint>,
    pub frame_expr: Option<CompiledExpr>,
}

/// Guard state for a sub-component's optional `where` clause.
///
/// Encodes the three valid states of a sub's termination guard, making the previously-
/// impossible state `(Some(_), guard_compile_failed=true)` unrepresentable.
#[derive(Debug, Clone)]
pub enum GuardState {
    /// The user wrote no `where` clause on this sub.
    None,
    /// The user wrote a `where` clause but it failed to compile (Severity::Error
    /// diagnostics were emitted during guard compilation). Used by the termination
    /// check to suppress the misleading "add a where clause" cascade error when the
    /// user already wrote one — it just did not compile.
    Broken,
    /// The user wrote a `where` clause that compiled successfully.
    Compiled(CompiledExpr),
}

impl GuardState {
    /// Returns the compiled guard expression if the user wrote a `where` clause
    /// that compiled successfully; `None` if there was no clause or it failed to compile.
    pub fn compiled(&self) -> Option<&CompiledExpr> {
        match self {
            GuardState::Compiled(g) => Some(g),
            _ => None,
        }
    }

    /// Returns `true` if the user wrote a `where` clause that compiled successfully.
    /// Equivalent to `self.compiled().is_some()`, but names the intent explicitly.
    pub fn is_compiled(&self) -> bool {
        matches!(self, GuardState::Compiled(_))
    }
}

/// A sub-component declaration — compiled from a SubDecl.
#[derive(Debug, Clone)]
pub struct SubComponentDecl {
    pub name: String,
    pub structure_name: String,
    pub visibility: Visibility,
    pub args: Vec<(String, CompiledExpr)>,
    /// Resolved type arguments for parameterized structures
    /// (e.g., `Box<Bolt>()` → `[StructureRef("Bolt")]`; `Box<U>()` → `[TypeParam("U")]`).
    pub type_args: Vec<Type>,
    /// True if this sub uses collection form: `sub name : List<T>`
    pub is_collection: bool,
    /// For collection subs, the synthetic count ValueCell (e.g. `__count_bolts`)
    pub count_cell: Option<ValueCellId>,
    /// Guard state for the sub's optional `where` clause.
    pub guard_state: GuardState,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A compiled guarded group — a set of members/constraints active only when a guard condition is true.
#[derive(Debug, Clone)]
pub struct CompiledGuardedGroup {
    /// The compiled guard condition expression.
    pub guard_expr: CompiledExpr,
    /// Synthetic ValueCellId for the guard (Bool, Let kind).
    pub guard_value_cell: ValueCellId,
    /// Members active when guard is true.
    pub members: Vec<ValueCellDecl>,
    /// Constraints active when guard is true.
    pub constraints: Vec<CompiledConstraint>,
    /// Members active when guard is false (else branch).
    pub else_members: Vec<ValueCellDecl>,
    /// Constraints active when guard is false (else branch).
    pub else_constraints: Vec<CompiledConstraint>,
    /// Parent guard ValueCellId for nested guards (None for top-level guards).
    /// Used to suppress false-positive cross-guard diagnostics when
    /// inner guard members reference outer guard members.
    pub parent_guard: Option<ValueCellId>,
}

/// A single arm of a match-block decl group (task 2372).
///
/// Produced by desugaring `match head_type { Hex => sub head : HexHead }` at decl
/// level — see PRD `docs/prds/match-block-decls.md` task 1 and spec §6.4.
///
/// The arm stores just the guard metadata needed for type narrowing (task 2373)
/// and union-type construction — the actual per-arm decl bodies are emitted into
/// the existing `value_cells` / `sub_components` collections under disambiguated
/// `ValueCellId`s and are not duplicated here.
#[derive(Debug, Clone)]
pub struct GuardedDeclArm {
    /// The compiled per-arm guard condition (e.g. `head_type == HeadType.Hex`).
    pub guard_expr: CompiledExpr,
    /// Synthetic `__guard_N` `ValueCellId` allocated by `compile_block_guard`.
    pub guard_value_cell: ValueCellId,
    /// The declared type of the arm's decl (e.g. `Type::StructureRef("HexHead")`).
    pub arm_type: Type,
}

/// A logical cluster of same-name declarations produced by a `match` block at
/// decl level (task 2372).
///
/// See PRD `docs/prds/match-block-decls.md` task 1 and spec §6.4.
/// Stored in `CompilationScope::match_arm_groups` — separate from the regular
/// `names` map so that future duplicate-name diagnostics (task 2375) cannot
/// misfire on cluster members.
///
/// **Exhaustiveness:** is *not* enforced here — a non-exhaustive `match`
/// compiles silently with the omitted variants having no arm guard. Spec §6.4
/// requires exhaustiveness; that check is scheduled for a follow-up task once
/// union typing (task 2373) lands and provides the type-level union to
/// compare patterns against.
#[derive(Debug, Clone)]
pub struct GuardedDeclGroup {
    /// The shared logical name of all arms (e.g. `"head"`).
    pub name: String,
    /// Per-arm metadata, one entry per `match` arm (or per `|`-pipe-collapsed arm).
    pub arms: Vec<GuardedDeclArm>,
}

/// A value cell declaration (param or let).
#[derive(Debug, Clone)]
pub struct ValueCellDecl {
    pub id: ValueCellId,
    pub kind: ValueCellKind,
    pub visibility: Visibility,
    pub cell_type: Type,
    pub default_expr: Option<CompiledExpr>,
    pub solver_hints: Vec<SolverHint>,
    pub span: SourceSpan,
}

/// A solver hint extracted from a `@solver_hint` annotation on a value cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolverHint {
    pub kind: SolverHintKind,
    pub collection: String,
    pub span: SourceSpan,
}

/// The kind of solver hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SolverHintKind {
    /// Restrict the value to a discrete set of values from the named collection.
    DiscreteSet,
    /// Prefer values from the named stock/standard collection.
    PreferStock,
    /// Advise the solver to use a named strategy when resolving this cell.
    ///
    /// The strategy name is an opaque ident stored in `SolverHint::collection`.
    /// ANY ident is accepted at compile time; the back-end emits a runtime warning
    /// if the strategy name is unrecognised, preserving the spec §12.2 advisory
    /// invariant (pragmas/hints are never compile errors).
    PreferredStrategy,
}

/// Whether a value cell is a parameter (externally settable), a let (computed),
/// or an auto parameter (solver-determined).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueCellKind {
    Param,
    Let,
    /// Solver-determined parameter: starts as Undef, value provided by constraint solver.
    /// `free`: when true this is an `auto(free)` parameter that skips uniqueness verification.
    Auto {
        free: bool,
    },
}

impl ValueCellKind {
    /// Returns `true` for any `Auto` variant (strict or free).
    pub fn is_auto(&self) -> bool {
        matches!(self, ValueCellKind::Auto { .. })
    }

    /// Returns `true` only for `Auto { free: true }`.
    pub fn is_auto_free(&self) -> bool {
        matches!(self, ValueCellKind::Auto { free: true })
    }
}

/// Visibility of a declaration: `Public` if accessible from outside, `Private` if internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Private,
}

/// A compiled constraint.
#[derive(Debug, Clone)]
pub struct CompiledConstraint {
    pub id: ConstraintNodeId,
    pub label: Option<String>,
    pub expr: CompiledExpr,
    pub span: SourceSpan,
    /// Optional pre-classified constraint domain. When `None`, the
    /// classifier determines the domain at solve time.
    pub domain: Option<ConstraintDomain>,
    /// Optional optimization target extracted from the originating
    /// `@optimized("target")` annotation on the source `constraint def`.
    ///
    /// When `Some`, the Engine's `dispatch_constraints` helper looks up the
    /// target in its optimization registry and, if a matching `OptimizedImpl`
    /// is registered, routes this constraint to that implementation instead
    /// of the language-level `ConstraintChecker`. When `None` (the default),
    /// the language-level checker handles the constraint as before.
    ///
    /// **Scope (Task 273):** this field is consumed only by the Engine's
    /// *checker* path (`dispatch_constraints`). The *solver* path
    /// (`ConstraintSolver::solve`, driven by `Engine::resolve`) currently
    /// feeds every constraint through the ordinary language-level solver,
    /// including `@optimized`-annotated ones — it does not yet branch on
    /// this field. A follow-up will extend the solver seam to route through
    /// an `OptimizedImpl` as well.
    pub optimized_target: Option<String>,
}

/// A realization declaration — specifies geometry to produce.
#[derive(Debug, Clone)]
pub struct RealizationDecl {
    pub id: RealizationNodeId,
    /// The user-facing name for this realization.
    ///
    /// The compiler always emits `Some(name)` for every `RealizationDecl` it
    /// produces.  `name` is either the let-binding name (`let body =
    /// cylinder(r, h)` → `"body"`) or the Solid-typed param name (`param body:
    /// Solid = ...` → `"body"`), including guarded-group Solid params handled
    /// by `emit_guarded_geometry_realizations`.
    ///
    /// `None` only arises from the
    /// `TopologyTemplateBuilder::realization(...)` test-support helper in
    /// `crates/reify-test-support/src/builders/topology.rs`.  Tests that need
    /// a user-visible name use `realization_named(...)` instead.
    pub name: Option<String>,
    pub operations: Vec<CompiledGeometryOp>,
    /// Feature tags parallel to `operations` — same length, same indexing.
    ///
    /// **Invariant**: `feature_tags.len() == operations.len()`.  Enforced
    /// via `debug_assert!` at construction sites (all of which call
    /// `derive_feature_tags`).  Tests in `feature_tag_tests.rs` lock this
    /// invariant against future refactors.
    pub feature_tags: Vec<reify_types::FeatureTag>,
    pub span: SourceSpan,
}

/// A compiled geometry operation.
#[derive(Debug, Clone)]
pub enum CompiledGeometryOp {
    /// Create a primitive shape.
    Primitive {
        kind: PrimitiveKind,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Boolean operation on two geometry refs.
    Boolean {
        op: BooleanOp,
        left: GeomRef,
        right: GeomRef,
    },
    /// Modify a shape (fillet, chamfer).
    Modify {
        kind: ModifyKind,
        target: GeomRef,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Transform a shape (translate, rotate).
    Transform {
        kind: TransformKind,
        target: GeomRef,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Pattern a shape (linear, circular, mirror).
    Pattern {
        kind: PatternKind,
        target: GeomRef,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Sweep operation (loft).
    Sweep {
        kind: SweepKind,
        profiles: Vec<GeomRef>,
        args: Vec<(String, CompiledExpr)>,
    },
    /// Curve construction (line_segment, arc, helix, interp, bezier, nurbs).
    Curve {
        kind: CurveKind,
        args: Vec<(String, CompiledExpr)>,
    },
}

/// Primitive geometry kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveKind {
    Box,
    Cylinder,
    Sphere,
    /// Hollow cylinder: `tube(outer_r, inner_r, height)`. Composed at the
    /// kernel layer as `boolean_cut` between two cylinders.
    Tube,
}

impl std::fmt::Display for PrimitiveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrimitiveKind::Box => f.write_str("box"),
            PrimitiveKind::Cylinder => f.write_str("cylinder"),
            PrimitiveKind::Sphere => f.write_str("sphere"),
            PrimitiveKind::Tube => f.write_str("tube"),
        }
    }
}

/// Boolean geometry operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}

impl std::fmt::Display for BooleanOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BooleanOp::Union => f.write_str("union"),
            BooleanOp::Difference => f.write_str("difference"),
            BooleanOp::Intersection => f.write_str("intersection"),
        }
    }
}

/// Modification operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifyKind {
    Fillet,
    Chamfer,
    Shell,
    Draft,
    Thicken,
}

impl ModifyKind {
    /// Every variant of this enum, as a fixed-size const array.
    ///
    /// This is the **single source of truth** for the set of `ModifyKind` variants.
    /// `VARIANT_COUNT` is derived from `ALL.len()`, so it cannot independently drift
    /// from this list.
    ///
    /// **Maintenance contract**: to add a new variant, extend this array (add the new
    /// element) and bump its explicit size annotation from `[Self; N]` to `[Self; N+1]`.
    /// Rust rejects any length mismatch at compile time, so the size annotation is itself
    /// an additional tripwire.  Once you bump the size, `VARIANT_COUNT` auto-updates, and
    /// `const _: () = assert!(CASES.len() == ModifyKind::VARIANT_COUNT, ...)` in
    /// `geometry_modify::single_geom_target_kinds()` fires at `cargo check`, forcing the
    /// matching `CASES` row to be added.
    const ALL: [Self; 5] = [
        Self::Fillet,
        Self::Chamfer,
        Self::Shell,
        Self::Draft,
        Self::Thicken,
    ];

    /// Count of variants — derived from `ALL.len()`, not hand-maintained.
    ///
    /// Cannot independently drift from `ALL` because it is `Self::ALL.len()`.  Consumer:
    /// `geometry_modify::single_geom_target_kinds()` uses `const _: () = assert!(CASES.len()
    /// == ModifyKind::VARIANT_COUNT, ...)` to lock `CASES` coverage at compile time.
    pub const VARIANT_COUNT: usize = Self::ALL.len();
}

impl std::fmt::Display for ModifyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModifyKind::Fillet => f.write_str("fillet"),
            ModifyKind::Chamfer => f.write_str("chamfer"),
            ModifyKind::Shell => f.write_str("shell"),
            ModifyKind::Draft => f.write_str("draft"),
            ModifyKind::Thicken => f.write_str("thicken"),
        }
    }
}

/// Transform operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformKind {
    Translate,
    Rotate,
    Scale,
    RotateAround,
}

impl std::fmt::Display for TransformKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransformKind::Translate => f.write_str("translate"),
            TransformKind::Rotate => f.write_str("rotate"),
            TransformKind::Scale => f.write_str("scale"),
            TransformKind::RotateAround => f.write_str("rotate_around"),
        }
    }
}

/// Pattern operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatternKind {
    Linear,
    Circular,
    Mirror,
    Linear2D,
    Arbitrary,
}

impl std::fmt::Display for PatternKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternKind::Linear => f.write_str("linear"),
            PatternKind::Circular => f.write_str("circular"),
            PatternKind::Mirror => f.write_str("mirror"),
            PatternKind::Linear2D => f.write_str("linear_2d"),
            PatternKind::Arbitrary => f.write_str("arbitrary"),
        }
    }
}

/// Sweep operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SweepKind {
    Loft,
    Extrude,
    Revolve,
    Sweep,
    /// Symmetric extrude: distance/2 each way from the profile.
    ExtrudeSymmetric,
    /// Sweep with an auxiliary guide wire for orientation control.
    SweepGuided,
    /// Loft through multiple sections with one or more guide wires.
    LoftGuided,
    /// Circular cross-section sweep along a path: `pipe(path, radius)`.
    /// Composed at the kernel layer as `make_pipe(make_circle_face(radius,
    /// 0.0), path)`.
    Pipe,
}

impl std::fmt::Display for SweepKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SweepKind::Loft => f.write_str("loft"),
            SweepKind::Extrude => f.write_str("extrude"),
            SweepKind::Revolve => f.write_str("revolve"),
            SweepKind::Sweep => f.write_str("sweep"),
            SweepKind::ExtrudeSymmetric => f.write_str("extrude_symmetric"),
            SweepKind::SweepGuided => f.write_str("sweep_guided"),
            SweepKind::LoftGuided => f.write_str("loft_guided"),
            SweepKind::Pipe => f.write_str("pipe"),
        }
    }
}

/// Curve construction operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurveKind {
    LineSegment,
    Arc,
    Helix,
    InterpCurve,
    BezierCurve,
    NurbsCurve,
}

impl std::fmt::Display for CurveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CurveKind::LineSegment => f.write_str("line_segment"),
            CurveKind::Arc => f.write_str("arc"),
            CurveKind::Helix => f.write_str("helix"),
            CurveKind::InterpCurve => f.write_str("interp_curve"),
            CurveKind::BezierCurve => f.write_str("bezier_curve"),
            CurveKind::NurbsCurve => f.write_str("nurbs_curve"),
        }
    }
}

/// Reference to a geometry result within a realization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GeomRef {
    /// Result of a previous operation (by index in the operations list).
    Step(usize),
    /// A sub-component's geometry output.
    Sub(String),
}

/// A compiled unit — the public output representation in `CompiledModule`.
#[derive(Debug, Clone)]
pub struct CompiledUnit {
    pub name: String,
    pub is_pub: bool,
    pub dimension: DimensionVector,
    pub factor: f64,
    pub offset: Option<f64>,
    pub content_hash: ContentHash,
}

/// A compiled type alias — the public output representation in `CompiledModule`.
///
/// Contains only semantic data (no `TypeExpr` from `reify_syntax`), preserving
/// the module boundary: downstream crates consuming `CompiledModule` do not
/// transitively depend on `reify_syntax`.
#[derive(Debug, Clone)]
pub struct CompiledTypeAlias {
    pub name: String,
    /// The resolved type for non-parameterized aliases; `None` for parameterized aliases.
    pub resolved_type: Option<Type>,
    /// Type parameters for parameterized aliases (empty for simple aliases).
    pub type_params: Vec<reify_types::TypeParam>,
    pub is_pub: bool,
    pub span: SourceSpan,
    pub content_hash: ContentHash,
}

/// A compiled parameter in a constraint definition.
///
/// Produced by `compile_constraint_def`; replaces the raw `reify_syntax::ParamDecl`
/// in the registry so entity scopes only see resolved data.
#[derive(Debug, Clone)]
pub struct CompiledConstraintParam {
    pub name: String,
    /// Original default expression, kept as AST so it can be substituted into the
    /// calling entity's scope at instantiation time.
    pub default: Option<reify_syntax::Expr>,
    pub span: SourceSpan,
}

/// A compiled constraint definition — produced once per `constraint def` declaration.
///
/// Removes the raw `reify_syntax::ConstraintDef` from `CompiledModule`, replacing it
/// with a struct whose fields are either fully resolved (annotations, params) or kept
/// as AST only where call-site context is required (predicates, param defaults).
#[derive(Debug, Clone)]
pub struct CompiledConstraintDef {
    pub name: String,
    /// `true` if declared with the `pub` modifier (exported to importing modules).
    pub is_pub: bool,
    /// Type parameters declared on this constraint def (e.g., `<T: Rigid>`).
    pub type_params: Vec<reify_types::TypeParam>,
    /// Parameters of the constraint def, compiled from `ParamDecl`s.
    pub params: Vec<CompiledConstraintParam>,
    /// Predicates kept as AST: param substitution is call-site-local, and compilation
    /// requires the calling entity's scope. This keeps the public type boundary clean
    /// (no `reify_syntax::ConstraintDef` in `CompiledModule`) while deferring full lowering.
    pub predicates: Vec<reify_syntax::Expr>,
    pub span: SourceSpan,
    pub content_hash: reify_types::ContentHash,
    /// Block-level pragmas from the parsed declaration.
    pub pragmas: Vec<reify_syntax::Pragma>,
    /// Lowered annotations (validated at def-compile time).
    pub annotations: Vec<reify_types::Annotation>,
    /// Cached `@optimized("target")` value extracted from `annotations` once at
    /// def-compile time so every instantiation can read it without re-scanning.
    pub annotations_optimized_target: Option<String>,
}

impl CompiledConstraintDef {
    /// Returns `true` if this constraint def is tagged with the `@test` annotation.
    pub fn is_test(&self) -> bool {
        reify_types::annotation::has_test_annotation(&self.annotations)
    }
}

#[cfg(test)]
mod kind_display_tests {
    //! Display-impl round-trip tests for op-kind enums.
    //!
    //! These strings are a user-facing contract — they appear in compiler
    //! diagnostics and are asserted on by `geometry_ops` /
    //! `geometry_error_handling` tests (`diagnostics[0].message.contains("box")`
    //! etc.). The exhaustive `match self` inside each `impl Display` is already
    //! compiler-enforced to cover every variant (Rust E0004), so these tests
    //! only need to pin the per-variant *string values*; a table-driven form
    //! keeps adding a new variant to a one-line row addition.
    use super::*;

    fn check<T: std::fmt::Display>(cases: &[(T, &str)]) {
        for (variant, expected) in cases {
            assert_eq!(format!("{}", variant), *expected);
        }
    }

    #[test] fn primitive_kind_display() { check(&[
        (PrimitiveKind::Box, "box"),
        (PrimitiveKind::Cylinder, "cylinder"),
        (PrimitiveKind::Sphere, "sphere"),
        (PrimitiveKind::Tube, "tube"),
    ]); }

    #[test] fn boolean_op_display() { check(&[
        (BooleanOp::Union, "union"),
        (BooleanOp::Difference, "difference"),
        (BooleanOp::Intersection, "intersection"),
    ]); }

    #[test] fn modify_kind_display() { check(&[
        (ModifyKind::Fillet, "fillet"),
        (ModifyKind::Chamfer, "chamfer"),
        (ModifyKind::Shell, "shell"),
        (ModifyKind::Draft, "draft"),
        (ModifyKind::Thicken, "thicken"),
    ]); }

    #[test] fn transform_kind_display() { check(&[
        (TransformKind::Translate, "translate"),
        (TransformKind::Rotate, "rotate"),
        (TransformKind::Scale, "scale"),
        (TransformKind::RotateAround, "rotate_around"),
    ]); }

    #[test] fn pattern_kind_display() { check(&[
        (PatternKind::Linear, "linear"),
        (PatternKind::Circular, "circular"),
        (PatternKind::Mirror, "mirror"),
        (PatternKind::Linear2D, "linear_2d"),
        (PatternKind::Arbitrary, "arbitrary"),
    ]); }

    #[test] fn sweep_kind_display() { check(&[
        (SweepKind::Loft, "loft"),
        (SweepKind::Extrude, "extrude"),
        (SweepKind::Revolve, "revolve"),
        (SweepKind::Sweep, "sweep"),
        (SweepKind::ExtrudeSymmetric, "extrude_symmetric"),
        (SweepKind::SweepGuided, "sweep_guided"),
        (SweepKind::LoftGuided, "loft_guided"),
        (SweepKind::Pipe, "pipe"),
    ]); }

    #[test] fn curve_kind_display() { check(&[
        (CurveKind::LineSegment, "line_segment"),
        (CurveKind::Arc, "arc"),
        (CurveKind::Helix, "helix"),
        (CurveKind::InterpCurve, "interp_curve"),
        (CurveKind::BezierCurve, "bezier_curve"),
        (CurveKind::NurbsCurve, "nurbs_curve"),
    ]); }
}

#[cfg(test)]
mod find_template_tests {
    use super::find_template;

    /// The None branch: passing an empty slice (or a slice where no name matches)
    /// must return None, not panic. This pins the contract that `find_template` is
    /// safe to call on any slice regardless of contents.
    #[test]
    fn missing_name_returns_none() {
        assert!(find_template(&[], "absent").is_none());
    }
}

#[cfg(test)]
mod guarded_decl_group_tests {
    //! Tests for `GuardedDeclArm` and `GuardedDeclGroup` structs (task 2372, step-1).
    //! RED until the structs are added in step-2.
    use super::*;
    use reify_types::Value;

    #[test]
    fn guarded_decl_group_struct_carries_name_and_arms() {
        let arm0 = GuardedDeclArm {
            guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
            guard_value_cell: ValueCellId::new("Bolt", "__guard_0"),
            arm_type: Type::StructureRef("HexHead".to_string()),
        };
        let arm1 = GuardedDeclArm {
            guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
            guard_value_cell: ValueCellId::new("Bolt", "__guard_1"),
            arm_type: Type::StructureRef("SocketHead".to_string()),
        };
        let g = GuardedDeclGroup {
            name: "head".to_string(),
            arms: vec![arm0, arm1],
        };
        assert_eq!(g.name, "head");
        assert_eq!(g.arms.len(), 2);
        assert_eq!(
            g.arms[0].guard_value_cell,
            ValueCellId::new("Bolt", "__guard_0")
        );
        assert_eq!(
            g.arms[1].arm_type,
            Type::StructureRef("SocketHead".to_string())
        );
    }
}
