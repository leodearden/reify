use std::fmt;

/// A byte-offset span in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    /// Byte offset of the start of the span.
    pub start: u32,
    /// Byte offset of the end of the span (exclusive).
    pub end: u32,
}

impl SourceSpan {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    pub fn empty(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// The raw `usize` byte-offset value that identifies the prelude sentinel.
    ///
    /// Equals `u32::MAX as usize` — the value you get when casting
    /// `SourceSpan::prelude().start` or `SourceSpan::prelude().end` to `usize`.
    /// Both `reify_core::byte_offset_to_line_col` and
    /// `gui::engine::offset_to_line_col_fast` check for this exact value and
    /// return `(1, 1)` without further computation.
    ///
    /// Prefer this constant over a bare `u32::MAX as usize` literal so the
    /// sentinel contract is expressed in one canonical location.
    pub const PRELUDE_SENTINEL_OFFSET: usize = u32::MAX as usize;

    /// A sentinel span used for prelude-originated entries that have no
    /// meaningful byte-offset in the current compilation unit.
    ///
    /// The value `{ start: u32::MAX, end: u32::MAX }` is guaranteed to fall
    /// outside any real source (files are bounded well below 4 GiB in
    /// practice).  Use [`SourceSpan::is_prelude`] to detect this sentinel
    /// before converting offsets to line/column positions.
    ///
    /// # Renderer behaviour
    ///
    /// - Both `reify_types::byte_offset_to_line_col` and the GUI/LSP fast path
    ///   (`gui::engine::offset_to_line_col_fast`) short-circuit the prelude
    ///   sentinel ([`SourceSpan::PRELUDE_SENTINEL_OFFSET`]) to `(1, 1)` — the
    ///   same "no user-file location" fallback used by `mcp_context::get_diagnostics`
    ///   when no labels are present.  This prevents a `debug_assert` panic
    ///   (debug builds) and a silent past-last-line mis-report (release builds).
    /// - Ad-hoc offset converters that do **not** route through one of those
    ///   helpers (e.g. `reify_lsp::convert::offset_to_position`) apply
    ///   `offset.min(source.len())` clamping instead, producing an EOF-position
    ///   rather than `(1, 1)`.  Callers using such converters must guard with
    ///   [`SourceSpan::is_prelude`] before the offset conversion.
    /// - The provenance truth for prelude-originated entries is carried by the
    ///   label *message* (e.g. "defined in stdlib prelude"), not the span
    ///   coordinates.  For explicit control over presentation, check
    ///   [`SourceSpan::is_prelude`] and substitute a "no user-file location"
    ///   message rather than relying on any numeric fallback.
    pub fn prelude() -> Self {
        Self {
            start: u32::MAX,
            end: u32::MAX,
        }
    }

    /// Returns `true` if this span is the prelude sentinel produced by
    /// [`SourceSpan::prelude`].
    pub fn is_prelude(&self) -> bool {
        self.start == u32::MAX && self.end == u32::MAX
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: SourceSpan) -> SourceSpan {
        SourceSpan {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
// Explicit rename keeps wire format PascalCase even if a future variant uses a non-PascalCase identifier.
#[cfg_attr(feature = "serde", serde(rename_all = "PascalCase"))]
pub enum Severity {
    /// Informational note.
    Info,
    /// Warning — something suspicious but not an error.
    Warning,
    /// Error — prevents compilation or evaluation.
    Error,
}

impl Severity {
    /// Canonical wire/log format string for this severity.
    ///
    /// Returns `"Error"`, `"Warning"`, or `"Info"` (PascalCase).
    ///
    /// This is the **single source of truth** for how severity appears in
    /// `DiagnosticInfo.severity` (wire format) and in structured log fields.
    /// It MUST stay in lock-step with the `#[serde(rename_all = "PascalCase")]`
    /// derive on this enum — a feature-gated cross-check in the inline tests
    /// (`#[cfg(feature = "serde")]`) is pinned by a unit test.
    ///
    /// Note: `Display` intentionally keeps lowercase (`"error"`, `"warning"`,
    /// `"info"`) for CLI/human-readable output. Do not change `Display` to
    /// PascalCase — that would silently alter the MCP CLI wire format.
    #[inline]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
            Severity::Info => "Info",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// Typed identifier for the *kind* of diagnostic emitted, decoupled from the
/// human-readable message text.
///
/// Test assertions and downstream tooling (e.g. the MCP wire layer) match on
/// `DiagnosticCode` rather than on substrings of `Diagnostic.message`, so
/// reword-the-message changes do not break tests or downstream consumers.
///
/// `#[non_exhaustive]` lets future variants be added without breaking external
/// match exhaustiveness. The serde derives are feature-gated to mirror the
/// `Severity` enum, and `rename_all = "PascalCase"` keeps the wire identifier
/// stable (e.g. `"TraitNotImplemented"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "PascalCase"))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// Origin: `crates/reify-compiler/src/expr.rs` (instance qualified-access).
    /// Replaces canonical message:
    /// `"sub-component '<name>' (type '<structure>') does not implement trait '<trait>'"`.
    TraitNotImplemented,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs::collect_all_requirements`.
    /// Replaces canonical message: `"unresolved trait: '<name>'"`.
    UnresolvedTrait,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs::collect_all_requirements`.
    /// Replaces canonical message:
    /// `"trait refinement chain too deep (exceeded <N> levels) at '<trait>'"`.
    TraitRefinementChainTooDeep,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Param/Let path).
    /// Replaces canonical message:
    /// `"conflicting trait requirements for '<name>': trait '…' requires …, trait '…' requires …"`.
    ConflictingTraitRequirements,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Sub path).
    /// Replaces canonical message:
    /// `"conflicting trait sub requirements for '<name>': trait '…' requires sub '…', …"`.
    ConflictingTraitSubRequirements,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Let-binding default conflict).
    /// Replaces canonical message:
    /// `"conflicting trait let bindings for '<name>': trait '…' and trait '…' provide different expressions"`.
    ConflictingTraitLetBindings,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Param/Constraint default conflict).
    /// Replaces canonical message:
    /// `"conflicting trait defaults for '<name>': trait '…' has …, trait '…' has …"`.
    ConflictingTraitDefaults,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "missing required member …".
    MissingRequiredMember,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "missing required sub-component …".
    MissingRequiredSubComponent,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "type mismatch for trait member …".
    TypeMismatchForTraitMember,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "type does not conform to trait …".
    TypeNotConformingToTrait,
    /// Origin: `crates/reify-compiler/src/conformance/mod.rs` (call-site Bounded check
    /// at trait-typed parameters of `Type::Geometry` arguments — task 2312).
    /// Canonical message form:
    /// `"geometry argument '<name>' is not Bounded; required by trait parameter"`.
    ///
    /// Emitted when a parameter has a trait-object type whose required trait is
    /// `Bounded` (the compile-inferred geometry trait declared in
    /// `crates/reify-compiler/stdlib/geometry_traits.ri`) and the argument's
    /// inferred [`InferredTraits`](../../reify_compiler/geometry_traits_inference/struct.InferredTraits.html)
    /// set lacks `bounded`. The PRD-prose mnemonic is `E_GEOMETRY_UNBOUNDED`
    /// (see `docs/prds/geometry-traits.md` §"Architectural decisions" point 2).
    ///
    /// Reserved for the Bounded case only. `Connected` and `Convex` violations
    /// at the same call-site shape reuse [`TypeNotConformingToTrait`] per the
    /// task's design decision §2.
    GeometryUnbounded,
    /// Origin: `crates/reify-constraints/src/lib.rs::SimpleConstraintChecker::check`.
    /// Replaces canonical messages:
    /// - `"constraint <id> violated"` (Bool(false) branch, Severity::Error)
    /// - `"constraint <id> evaluated to non-boolean value"` (non-bool fallback, Severity::Error)
    ///
    /// Note: Both the Bool(false) case (semantically violated predicate) and the non-bool
    /// fallback (expression is not a predicate at all) intentionally share this code. Both
    /// set `Satisfaction::Violated` and `Severity::Error`. If downstream tooling needs to
    /// distinguish "predicate returned false" from "expression was not boolean", a separate
    /// `ConstraintNotBoolean` variant can be added additively (the `#[non_exhaustive]` flag
    /// makes that non-breaking).
    ConstraintViolated,
    /// Origin: `crates/reify-constraints/src/lib.rs::SimpleConstraintChecker::check`.
    /// Replaces canonical message:
    /// `"constraint <id> indeterminate: undefined inputs"` (Undef branch, Severity::Warning)
    ConstraintIndeterminate,
    /// Origin: `crates/reify-constraints/src/solver.rs::DimensionalSolver`,
    ///          `crates/reify-constraints/src/solvespace.rs::SolveSpaceSolver`, and
    ///          `crates/reify-constraints/src/cpsat.rs::CpSatSolver`.
    /// Replaces canonical messages:
    /// - `"constraints could not be satisfied (max absolute residual: …)"` (solver.rs, Severity::Error)
    /// - `"geometric constraints are inconsistent (<n> failed)"` (solvespace.rs, Severity::Error)
    /// - `"CpSatSolver: no satisfying assignment found for … auto params with … constraints"` (cpsat.rs, Severity::Error)
    ConstraintUnsatisfiable,
    /// Origin: `crates/reify-constraints/src/solver.rs::DimensionalSolver`
    ///          (strict-auto uniqueness verification path, `verify_uniqueness`).
    /// Replaces canonical message:
    /// `"strict auto parameter resolution is not uniquely determined — consider using auto(free) for exploration"`.
    ///
    /// Semantically distinct from [`ConstraintUnsatisfiable`]: non-uniqueness means *multiple*
    /// valid solutions exist (the system is underdetermined), not zero. A strictly-auto
    /// parameter requires a unique solution; this code is emitted when perturbation-based
    /// uniqueness checking finds a second distinct solution.
    ConstraintNonUnique,
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_field`.
    /// Emitted when a field declaration uses the `sampled { ... }` source form,
    /// which is deferred to v0.2 (v0.1 supports `analytical` and `composed` only).
    FieldSampledV02,
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_field`.
    /// Emitted when a field declaration uses the `imported { ... }` source form,
    /// which is deferred to v0.2 (v0.1 supports `analytical` and `composed` only).
    FieldImportedV02,
    /// Origin: `crates/reify-expr/src/sampled.rs::sample_at_point`.
    /// Emitted as a `Severity::Warning` once per Sampled field per session
    /// when a `sample(field, point)` query falls outside the configured
    /// `BoundingBox` bounds; the result is `Value::Undef`.
    ///
    /// Canonical message form:
    /// `"sampled field '<name>' query is out of bounds; returning Undef"`.
    ///
    /// The PRD-prose mnemonic for this code is `W_FIELD_OUT_OF_BOUNDS`.
    /// Once-per-field-per-session emission is enforced by an `AtomicBool`
    /// `oob_emitted` flag on the runtime `SampledField` value
    /// (see `crates/reify-types/src/value.rs::SampledField`).
    FieldOutOfBounds,
    /// Origin: `crates/reify-eval/src/engine_eval.rs::build_sampled_field`.
    /// Emitted as a `Severity::Warning` when a `sampled` field's runtime
    /// config fails to parse (typo'd grid kind, wrong interpolation name,
    /// non-string slot for a string-keyed key, non-list `data`, etc.) or
    /// violates a runtime invariant required by the interpolation primitives
    /// (mismatched `data` length, axis grid with fewer than 2 nodes,
    /// non-positive or non-finite spacing).
    ///
    /// On emission the field's lambda becomes `Value::Undef` and any
    /// `sample(...)` call returns `Undef` — the warning gives the user a
    /// clear message naming the field, the offending value, and (where
    /// applicable) the allowed-set hint, instead of letting
    /// `interp::interpolate_Nd`'s `assert!` panic the eval loop.
    ///
    /// Canonical message form:
    /// `"sampled field '<name>': invalid <key>: expected <hint>, got <short_value>"`
    /// (parse failure) or
    /// `"sampled field '<name>': data length <N> does not match grid shape (<...>); expected <M> elements"`
    /// (runtime invariant violation).
    ///
    /// The PRD-prose mnemonic for this code is `W_FIELD_SAMPLED_INVALID_CONFIG`.
    /// Severity is `Warning` (not `Error`) for consistency with the sibling
    /// `W_FIELD_OUT_OF_BOUNDS` and `W_INTERPOLATION_DEFERRED` warnings emitted
    /// from the same dispatch path; downstream tooling that wants to surface
    /// these as harder failures can filter by code at the consumer side.
    FieldSampledInvalidConfig,
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_field`.
    /// Replaces canonical message:
    /// `"field '<name>' codomain mismatch: declared codomain '<C>', lambda body produces '<T>'"`.
    ///
    /// Emitted when the inferred type of an `analytical` lambda body does not
    /// implicitly convert to the declared codomain type. The human-readable
    /// mnemonic used in PRD prose is `E_FIELD_CODOMAIN_MISMATCH`.
    FieldCodomainMismatch,
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_function`.
    ///
    /// Canonical message form:
    /// `"function '<fn>' param '<p>' default type mismatch: declared param type '<P>', default expression produces '<D>'"`.
    ///
    /// Emitted when the compiled default expression for a function parameter has a
    /// `result_type` that does not exactly equal the resolved parameter type. The check
    /// uses strict equality (matching the policy used by `resolve_function_overload` and
    /// `try_default_padding`'s prefix check) rather than bidirectional `type_compatible`.
    /// The diagnostic is anchored to the default expression's span so the user sees the
    /// offending literal or sub-expression, not just the param declaration.
    ///
    /// The human-readable mnemonic used in task prose is `E_FN_PARAM_DEFAULT_TYPE_MISMATCH`.
    FnParamDefaultTypeMismatch,
    /// Origin: `crates/reify-compiler/src/compile_builder/dot_chain_lint.rs`.
    /// Emitted as a Warning when a left-associative `MemberAccess` chain in
    /// the parsed AST exceeds the configured depth threshold (currently
    /// `DEEP_DOT_CHAIN_THRESHOLD = 4`). Implements spec §5.7's
    /// "deep dot-chain" lint: counts `.field` hops only, treats
    /// `IndexAccess`/`FunctionCall`/`EnumAccess` and any other
    /// non-`MemberAccess` expression as a fresh chain root.
    DeepDotChain,
    /// Origin: `crates/reify-expr/src/interp.rs`.
    /// Emitted as a Warning when an interpolation call selects
    /// `InterpolationMethod::Rbf` or `InterpolationMethod::Kriging`, both of
    /// which are deferred to post-v0.1. The call falls back to
    /// `InterpolationMethod::Linear` and emits a single diagnostic of the form:
    ///
    /// `"interpolation method '<RBF|Kriging>' is deferred to post-v0.1; falling back to Linear"`.
    ///
    /// The PRD-prose mnemonic for this code is `W_INTERPOLATION_DEFERRED`.
    InterpolationDeferred,
    /// Origin: `crates/reify-compiler/src/expr.rs` (binary-op `Add`/`Sub` site and
    ///          range-bounds site), via `crates/reify-compiler/src/type_compat::format_dimension_mismatch_diagnostic`.
    /// Canonical message form: `"dimension mismatch in {op}: {left} vs {right}"`.
    ///
    /// Emitted when two `Type::Scalar` operands carry different, incompatible
    /// dimensions (e.g. Money vs Force). The diagnostic may carry an optional
    /// secondary label naming the canonical dimensions when both are known
    /// (e.g. `"Money and Force are different dimensions and cannot be combined directly"`).
    DimensionMismatch,
    /// Origin: `crates/reify-compiler/src/compile_builder/shadow_lint.rs`.
    /// Emitted as a Warning when a child-scope binder (e.g. lambda parameter,
    /// quantifier-bound variable) uses the same name as a name visible from an
    /// enclosing parent scope.
    ///
    /// Canonical message form:
    /// `"declaration of '<name>' shadows enclosing declaration"`.
    ///
    /// Two labels accompany the warning: the child binder site
    /// (`"shadows the enclosing declaration"`) and the original parent decl site
    /// (`"originally declared here"`). The PRD-prose mnemonic for this code is
    /// `W_SHADOW`. See `docs/prds/shadowing-warning.md` and spec §8.5.
    Shadowing,
    /// Origin: `crates/reify-compiler/src/entity.rs` (trait_bound iteration).
    /// Canonical message form:
    /// `"geometry trait '<TraitName>' on '<EntityName>' is treated as a user assertion; runtime conformance check is suppressed"`.
    ///
    /// Emitted as a `Warning` once per `(structure_def, geometry_marker_bound)` pair
    /// when a structure (or occurrence) explicitly declares one of the seven stdlib
    /// geometry-conformance marker traits (`Bounded`, `Closed`, `Manifold`,
    /// `Orientable`, `Convex`, `Connected`, `Watertight`) in its `trait_bounds`
    /// list. The declaration is treated as a user assertion that bypasses any future
    /// runtime conformance check (PRD tasks 4/5 — OCCT BRepCheck hook — are not
    /// yet wired; today the warning is the only observable effect).
    ///
    /// Detection is name-based against the canonical seven (case-sensitive) — see
    /// [`crates/reify-compiler/src/geometry_traits.rs`]'s
    /// `is_geometry_marker_trait` helper and the design decision in task 2321.
    ///
    /// The PRD-prose mnemonic for this code is `W_TRAIT_USER_ASSERTED`
    /// (see `docs/prds/geometry-traits.md` §"Scope" point 5).
    TraitUserAsserted,
    /// Origin: `crates/reify-eval/src/topology_selectors.rs::resolve_unique_by_tag`.
    /// Emitted as a `Warning` when a feature-tag selector matches zero or multiple
    /// sub-shapes after a topology change (i.e. the unique-tag invariant is violated).
    ///
    /// Canonical message form:
    /// `"feature-tag selector matched <N> sub-shapes (expected exactly 1; topology may have changed)"`.
    ///
    /// Two labels accompany the warning: a primary label at the selector call site
    /// (`"selector call"`) and a secondary label at the `FeatureTag::source_span`
    /// of the target tag (`"feature originally produced here"`).
    ///
    /// The [`crate::FeatureTagTable`] that `resolve_unique_by_tag` reads from is
    /// populated by the four `*_with_tags` filter selectors in
    /// `crates/reify-eval/src/topology_selectors.rs`:
    ///   - `edges_at_height_with_tags` (task 2323)
    ///   - `edges_by_length_with_tags` (task 2329)
    ///   - `faces_by_area_with_tags` (task 2329)
    ///   - `edges_parallel_to_with_tags` (task 2329)
    ///
    /// Each populator records a tag for every extracted sub-shape before
    /// applying its filter predicate, so `resolve_unique_by_tag` can look up
    /// any extracted sub-shape, not just those that passed the predicate.
    ///
    /// The PRD-prose mnemonic for this code is `W_TOPOLOGY_TAG_STALE`
    /// (see `docs/prds/topology-selectors.md` task 6).
    TopologyTagStale,
    /// Origin: `crates/reify-eval/src/topology_attribute_resolver.rs::resolve_unique_by_attribute`.
    /// Emitted as a `Warning` when the v0.2 attribute-based selector resolver matches
    /// zero or multiple sub-shapes after a topology change (i.e. the unique-attribute
    /// invariant is violated for the supplied `AttributeQuery`), specifically for
    /// genuine-ambiguity outcomes (zero-match or mixed parent-keys).
    ///
    /// Canonical message form:
    ///   - `"topology-attribute selector matched <N> sub-shapes (expected exactly 1; topology may have changed)"`
    ///     — emitted on a zero-match miss or a multi-match where the matched
    ///     candidates have MIXED parent-keys (genuine ambiguity, e.g. label
    ///     collision across distinct features). Resolution outcome:
    ///     `AttributeResolution::Unresolved`.
    ///
    /// Two labels accompany the warning where information is available:
    ///   - a primary label at the selector call site (`"selector call"`); and
    ///   - (optionally, when an originating `source_span` becomes available on
    ///     `TopologyAttribute` in a later task) a secondary label at the
    ///     originating-feature span (`"feature originally produced here"`).
    ///
    /// Today only the primary label is emitted because `TopologyAttribute` carries
    /// no `source_span` field.
    ///
    /// Coexists with [`TopologyTagStale`] during the v0.1→v0.2 migration window
    /// (see PRD `docs/prds/v0_2/persistent-naming-v2.md`). Distinct codes let
    /// test assertions and downstream tooling distinguish a v0.1 selector failure
    /// from a v0.2 attribute-resolver failure during the migration window.
    ///
    /// The split-cluster sub-form (post-split-cluster outcome) has its own typed
    /// variant: see [`TopologyAttributeAmbiguousAfterSplit`]. The local-index
    /// reassignment sub-form (ordering-shuffle rebind, no split) likewise has
    /// its own typed variant: see [`TopologyAttributeLocalIndexReassigned`].
    ///
    /// The PRD-prose mnemonic for this code is `W_TOPOLOGY_ATTRIBUTE_STALE`.
    TopologyAttributeStale,
    /// Origin: `crates/reify-eval/src/topology_attribute_resolver.rs::emit_split_children_diagnostic`.
    ///
    /// Emitted as a `Warning` when the v0.2 attribute resolver encounters a
    /// multi-match where ALL matched candidates share the same parent-key
    /// (`feature_id`, `role`, `local_index`, `user_label`) and differ only in
    /// `mod_history` — the signature of a post-split cluster. Resolution outcome:
    /// `AttributeResolution::AmbiguousAfterSplit { children }`.
    ///
    /// Canonical message form:
    ///   `"topology-attribute selector matched <N> split children of the same parent (disambiguate via split_by(...) selector once vocabulary v2 lands)"`
    ///
    /// Per PRD `docs/prds/v0_2/persistent-naming-v2.md` line 64, the resolver
    /// surfaces the children set for user disambiguation rather than silently
    /// rebinding. This is the typed disambiguation of the post-split-cluster
    /// outcome introduced in task #2653, distinct from the genuine-ambiguity
    /// case which retains [`TopologyAttributeStale`].
    ///
    /// A primary label is emitted at the selector call site (`"selector call"`).
    ///
    /// The PRD-prose mnemonic for this code is `W_TOPOLOGY_ATTRIBUTE_AMBIGUOUS_AFTER_SPLIT`.
    TopologyAttributeAmbiguousAfterSplit,
    /// Origin: `crates/reify-eval/src/topology_attribute_propagation.rs::detect_local_index_reassignment_diagnostics`.
    ///
    /// **Construction-time fragility detection (interim implementation).** The
    /// PRD-prose intent (line 72) is "emit when an existing selector's resolved
    /// topology changes after an edit purely due to ordering shuffle". A strict
    /// reading of that prose requires a *prior-vs-current* comparison across two
    /// builds. The current emitter is a forward-looking *risk* detector:
    /// constructed at populator time, it fires when two `(feature_id, role)`-
    /// peer entries have geometrically tied centroids within a kernel-epsilon
    /// tolerance — meaning the kernel's enumeration order is the only thing
    /// disambiguating their `local_index` assignment, and a future edit could
    /// shuffle them. So the variant currently warns that resolution **may**
    /// shuffle under a future edit, not that it **did** shuffle since a prior
    /// build. Cross-build delta comparison is recorded as a deferred follow-up
    /// (see task #2654 design decisions); this variant doc-comment will be
    /// updated when that lands.
    ///
    /// Canonical message form (current construction-time emitter):
    ///   `"topology-attribute selector for (feature '<feature_id>', role '<role>') has geometrically tied local_index assignments at indices <i> and <j>; selector resolution may shuffle after edits"`
    ///
    /// Per PRD `docs/prds/v0_2/persistent-naming-v2.md` line 72 ("Diagnostic
    /// on local_index reassignment"), the system surfaces ordering-shuffle
    /// rebinds rather than silently re-resolving. Symmetric splits (e.g.
    /// fillet of a full circular edge) accept arbitrary tiebreak with this
    /// diagnostic per PRD line 66.
    ///
    /// A primary label is emitted at the realization's source span
    /// (`"realization producing geometrically tied attributes"`); detection
    /// runs at realization-construction time, before any selector resolution.
    ///
    /// Distinct from [`TopologyAttributeAmbiguousAfterSplit`] (which covers
    /// post-split clusters where `mod_history` lengthens) and from
    /// [`TopologyAttributeStale`] (which covers genuine ambiguity / zero-match).
    ///
    /// The PRD-prose mnemonic for this code is `W_TOPOLOGY_ATTRIBUTE_LOCAL_INDEX_REASSIGNED`.
    TopologyAttributeLocalIndexReassigned,
    /// Origin: `crates/reify-compiler/src/compile_builder/specialization_scope_check.rs`.
    ///
    /// Emitted as an `Error` when a `param`, `port`, or `sub` declaration appears
    /// directly inside a specialization-scope body (`sub name : T { … }`).
    /// Specialization scopes (spec §8.7) permit only `let`, `constraint`, `connect`,
    /// `chain`, and similar override/binding forms — they may not introduce new
    /// structural members.
    ///
    /// Canonical message form:
    /// `"'<kind>' declaration '<name>' is not permitted in a specialization scope (spec §8.7)"`
    /// where `<kind>` is one of `param`, `port`, or `sub`.
    ///
    /// A single label accompanies the error at the offending declaration's span:
    /// `"forbidden in specialization scope"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_SPECIALIZATION_FORBIDDEN_DECL`
    /// (see `docs/prds/specialization-scope.md` and spec §8.7).
    SpecializationForbiddenDecl,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs`
    /// (Phase A candidate enumeration — `enumerate_candidates`).
    ///
    /// Canonical message form:
    /// `"auto type parameter has more than 10 candidates satisfying bound '<TraitNames>'; first 10 alphabetically: <names>"`.
    ///
    /// Emitted as `Severity::Error` when the pool of in-scope structures
    /// satisfying an `auto: TraitName` bound exceeds the cap of 10. The
    /// diagnostic carries the alphabetically-first 10 FQNs both in the
    /// human-readable message and in the structured
    /// [`Diagnostic::candidates`] field, so LSP / MCP consumers can read
    /// the list without parsing message text.
    ///
    /// The PRD-prose mnemonic for this code is `E_AUTO_TYPE_PARAM_POOL_OVERFLOW`
    /// (see `docs/prds/auto-type-param-resolution.md` §"Phase A").
    AutoTypeParamPoolOverflow,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs`
    /// (Phase C selection logic — `select_candidate`).
    ///
    /// Canonical message form:
    /// `"auto type parameter has no feasible candidates for bound '<TraitNames>': <rejection_summary>"`
    /// where `<rejection_summary>` lists each rejected candidate paired with
    /// the violated constraint id(s) (e.g.,
    /// `"'X' rejected by constraint <id>, 'Y' rejected by constraint <id>"`).
    ///
    /// Emitted as `Severity::Error` when Phase B's
    /// [`crate::Satisfaction`]-based feasibility filter rejects every
    /// candidate produced by Phase A. The diagnostic carries the rejected
    /// candidate FQNs in the structured [`Diagnostic::candidates`] field
    /// (input order, alphabetical) so LSP / MCP consumers can read the
    /// list without parsing message text. A single label is attached at
    /// the `auto:` use-site span.
    ///
    /// The PRD-prose mnemonic for this code is `E_AUTO_TYPE_PARAM_NO_CANDIDATE`
    /// (see `docs/prds/auto-type-param-resolution.md` §"Phase C").
    ///
    /// **Multi-param cross-product no-feasible (v0.2 backtracking).** When
    /// `resolve_auto_type_params_with_backtracking` exhausts the cross-product
    /// DFS with `feasible_assignments.is_empty()` (the `0 =>` arm), it emits
    /// a richer message in place of the v0.1 zero-rejections form. Canonical
    /// template:
    ///
    /// ```text
    /// auto type-parameter cross-product search found no feasible assignment
    /// for parameters [<names>]: candidates per parameter: <T=N, U=M, …>;
    /// cross-product size: <total>; depth: <n> (max_depth = <m>);
    /// first-param prefix illustration: <T=fqn> (lex-first level-1 prefix;
    /// sub-tree size <count>; entire cross-product is infeasible — no
    /// specific conflict localized)
    /// ```
    ///
    /// The "first-param prefix illustration" is **NOT conflict
    /// localization** — backjumping (task 2660) guarantees the entire
    /// cross-product is infeasible whenever this arm fires, so every level-1
    /// prefix is identically "infeasible". The illustration is a fixed-shape
    /// labeling anchor (lex-first level-1 prefix), and the message wording
    /// explicitly tells the user no specific conflict was localized so the
    /// illustration is not mistaken for a help-channel signal. True
    /// conflict-localization work (inspecting rejected leaves' violated
    /// constraints) is intentionally deferred.
    ///
    /// The structured [`Diagnostic::candidates`] field carries the **prefix
    /// illustration's FQN list** in declared parameter order (length 1 for
    /// the level-1 prefix — every multi-param cross-product no-feasible
    /// diagnostic collapses to a level-1 prefix post-backjumping; see PRD
    /// `docs/prds/v0_2/auto-resolution-backtracking.md` §"Resolved design
    /// decisions"). The bare FQN goes through the structured field; the
    /// human-readable `T=fqn` rendering with param-name pairing lives in
    /// the message only — preserving the FQN-only invariant on `candidates`
    /// (see field doc-comment). A single label is attached on
    /// `params[0].use_site_span` (first-param anchoring convention shared
    /// with v0.1 BFS strict-Ambiguous and post-2659 cross-product
    /// Ambiguous). Mirrors the multi-param shape under `AutoTypeParamAmbiguous`
    /// — single code, two message forms (v0.1 single-param vs. v0.2
    /// cross-product). Emission site:
    /// `crates/reify-compiler/src/auto_type_param.rs::emit_no_feasible_cross_product_diagnostic`.
    AutoTypeParamNoCandidate,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs`
    /// (Phase C selection logic — `select_candidate`).
    ///
    /// Canonical message form:
    /// `"auto type parameter has multiple feasible candidates for bound '<TraitNames>': <names>; consider an explicit substitution like '<lex_first>' instead of 'auto:'"`.
    ///
    /// Emitted as `Severity::Error` when, under strict (`free = false`)
    /// resolution, Phase B yields ≥2 feasible candidates. The diagnostic
    /// carries every feasible FQN in the structured
    /// [`Diagnostic::candidates`] field (input/alphabetical order) and
    /// surfaces the lexicographically-first FQN as the suggested explicit
    /// substitution in the human-readable message. A single label is
    /// attached at the `auto:` use-site span.
    ///
    /// The PRD-prose mnemonic for this code is `E_AUTO_TYPE_PARAM_AMBIGUOUS`
    /// (see `docs/prds/auto-type-param-resolution.md` §"Phase C").
    ///
    /// **Multi-param cross-product Ambiguous (v0.2 backtracking).** When
    /// `resolve_auto_type_params_with_backtracking` finds ≥2 feasible
    /// cross-product assignments under strict mode, the structured
    /// [`Diagnostic::candidates`] field carries the **lex-first feasible
    /// cross-product leaf's FQN list** (in declared parameter order),
    /// NOT the per-leaf composite witness summaries. Per-leaf witnesses
    /// (e.g. `"T=ORingSeal,U=AirCooled"`) appear only in the
    /// human-readable [`Diagnostic::message`] field. This preserves the
    /// FQN-only invariant on `candidates` (see field doc-comment) so LSP
    /// quick-fixes can offer the lex-first leaf as a coherent explicit
    /// substitution. Task 2663 (search-failure diagnostic format)
    /// inherits this contract.
    AutoTypeParamAmbiguous,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs`
    /// (Phase C selection logic — `select_candidate`).
    ///
    /// Canonical message form:
    /// `"auto(free) type parameter has multiple feasible candidates for bound '<TraitNames>': <names>; selected lexicographically-first '<lex_first>'"`.
    ///
    /// Emitted as `Severity::Warning` when, under `auto(free)` resolution,
    /// Phase B yields ≥2 feasible candidates. The diagnostic carries every
    /// feasible FQN in the structured [`Diagnostic::candidates`] field
    /// (input/alphabetical order) and names the lexicographically-first
    /// FQN — which Phase C selects — in the human-readable message.
    /// A single label is attached at the `auto:` use-site span.
    ///
    /// Severity is `Warning` (not `Error`) because `auto(free)` semantics
    /// permit the compiler to choose: the warning surfaces the choice for
    /// auditability without blocking compilation. This is the load-bearing
    /// distinction from `AutoTypeParamAmbiguous`.
    ///
    /// The PRD-prose mnemonic for this code is `W_AUTO_TYPE_PARAM_NON_UNIQUE`
    /// (see `docs/prds/auto-type-param-resolution.md` §"Phase C").
    AutoTypeParamNonUnique,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs::resolve_auto_type_params_with_backtracking`.
    ///
    /// Canonical message form:
    /// `"auto type-parameter search exceeded depth bound: <N> auto-type-params declared, max_depth = <M>; falling back to per-parameter BFS (v0.1 algorithm). NOTE: BFS-fallback soundness is contingent on Type::TypeParam → Type::StructureRef substitution remaining deferred; once the substitution pass lands, this fallback may silently pick wrong substitutions."`
    /// where `<N>` is `params.len()` and `<M>` is the configured `max_depth`.
    ///
    /// Emitted as `Severity::Warning` when the v0.2 DFS-over-cross-product
    /// algorithm receives more `auto:` type-parameters than the configured
    /// depth bound (`params.len() > max_depth`). The DFS falls back to the
    /// v0.1 per-parameter BFS (`resolve_auto_type_params`) immediately after
    /// emission, so the user always has a working compile — the warning is
    /// for auditability that the cross-product search did not run.
    ///
    /// Severity is `Warning` (not `Error`) because the fallback is
    /// functionally correct (BFS is sound, just less complete than DFS over
    /// the cross-product). The default `max_depth` is `6` per
    /// [`reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH`]; a single label is
    /// attached at the first param's `auto:` use-site span.
    ///
    /// The PRD-prose mnemonic for this code is
    /// `W_AUTO_TYPE_PARAM_DEPTH_BOUND_EXCEEDED` (see
    /// `docs/prds/v0_2/auto-resolution-backtracking.md` §"Resolved design
    /// decisions").
    AutoTypeParamDepthBoundExceeded,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs::resolve_auto_type_params_with_backtracking`.
    ///
    /// Canonical message form:
    /// `"auto type-parameter cross-product search exceeded size cap: <N> auto-type-params declared (<P1>, <P2>, ...) with per-param candidate counts [<k1>, <k2>, ...] yielding cross-product size <S>, max_cross_product_size = <C>; falling back to per-parameter BFS (v0.1 algorithm). NOTE: BFS-fallback soundness is contingent on Type::TypeParam → Type::StructureRef substitution remaining deferred; once the substitution pass lands, this fallback may silently pick wrong substitutions."`
    /// where `<N>` is `params.len()`, `<P*>` are the param names, `<k*>` are
    /// the per-param Phase A candidate counts, `<S>` is the computed
    /// cross-product size (`per_param_candidates.iter().map(|v| v.len()).fold(1, checked_mul)`),
    /// and `<C>` is the configured `max_cross_product_size`.
    ///
    /// Emitted as `Severity::Warning` when the v0.2 DFS-over-cross-product
    /// algorithm's per-param Phase A candidate enumeration completes
    /// successfully and the resulting cross-product size strictly exceeds
    /// the configured cap (`cross_product_size > max_cross_product_size`).
    /// The DFS falls back to the v0.1 per-parameter BFS
    /// (`resolve_auto_type_params`) immediately after emission, so the user
    /// always has a working compile — the warning is for auditability that
    /// the cross-product search did not run.
    ///
    /// Severity is `Warning` (not `Error`) because the fallback is
    /// functionally correct (BFS is sound, just less complete than DFS over
    /// the cross-product). The default `max_cross_product_size` is `100_000`
    /// per [`reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE`];
    /// a single label is attached at the first param's `auto:` use-site span
    /// (declared-order halt anchors on the first param — same convention as
    /// `AutoTypeParamDepthBoundExceeded`).
    ///
    /// The PRD-prose mnemonic for this code is
    /// `W_AUTO_TYPE_PARAM_CROSS_PRODUCT_SIZE_EXCEEDED` (see
    /// `docs/prds/v0_2/auto-resolution-backtracking.md` §"Resolved design
    /// decisions").
    AutoTypeParamCrossProductSizeExceeded,
    /// Origin: `crates/reify-compiler/src/traits.rs::compile_purpose` (Let arm).
    ///
    /// Canonical message form:
    /// `"let bindings in purpose bodies are not yet supported: '<name>'"`.
    ///
    /// Emitted as `Severity::Error` when `compile_purpose` encounters a
    /// `MemberDecl::Let` inside a purpose body. This is an *unsupported-feature*
    /// error — `CompiledPurpose` has no storage for let expressions, and
    /// `activate_purpose` only injects constraints. Any constraint referencing a
    /// let-bound name would produce a `ValueCellId` with no backing node in the
    /// eval graph. This is therefore not a `DuplicateDecl` error.
    ///
    /// A single label accompanies the error at the offending `let` declaration's
    /// span: `"unsupported in purpose"`.
    ///
    /// Design rationale for coexistence with `Shadowing`: when a purpose-body `let`
    /// also shadows a purpose param, both diagnostics fire at the same span — see
    /// `shadowing_warning_tests.rs::purpose_body_let_shadow_coexists_with_unsupported_let_error_intentional`.
    PurposeLetUnsupported,
    /// Origin: `crates/reify-stdlib/src/mechanism.rs` (task 2528 — `mechanism().body(...)`
    /// builder). Originally reserved for a closed-chain detector that would reject
    /// mechanisms whose joint-parent graph has a conflict (joint J recorded with two
    /// different parents) or a cycle (DFS reaches J again before reaching the world
    /// sentinel).
    ///
    /// **v0.2: not currently emitted — see `docs/prds/v0_2/kinematic-constraints.md`.**
    /// Closed kinematic chains are no longer treated as errors: the v0.2 mechanism
    /// builder (task 2671) records each closing edge as a `loop_closure` constraint
    /// in the Mechanism Map's `loop_closures` field and continues normal construction.
    /// The Mechanism Map shape no longer carries `error`, `error_path1`, `error_path2`,
    /// or `error_message` fields for closed-chain detection — closed chains are valid
    /// v0.2 mechanisms.
    ///
    /// The PRD-prose mnemonic for this code is `E_KINEMATIC_CLOSED_CHAIN`
    /// (see `docs/prds/kinematic-constraints.md` task 3 and
    /// `docs/reify-stdlib-reference.md` §13.2).
    ///
    /// The variant is RESERVED for a hypothetical future use case — for example, a
    /// user-opt-in strict mode (e.g. a purpose annotation rejecting closed chains) or
    /// a downstream consumer that wants to surface closed-chain detection as a
    /// diagnostic — but is NOT currently emitted by any path in the v0.2 builder.
    /// Removing the variant would require a wider refactor across reify-types and
    /// any downstream tooling (LSP / MCP / IDE error UIs) that pattern-matches on
    /// it; out of scope for the v0.2 closed-chain → loop-closure migration.
    KinematicClosedChain,
    /// Origin: `crates/reify-stdlib/src/mechanism.rs` (task 2528 — `mechanism().body(...)`
    /// builder). Reserved for the duplicate-solid detector that rejects a `body()` call
    /// whose `solid` argument equals (by structural `Value::Eq`) the `solid` of an
    /// already-recorded body in the same Mechanism.
    ///
    /// Canonical message form:
    /// `"duplicate solid in mechanism: solid already bound to body <id>"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_MECHANISM_DUPLICATE_SOLID`
    /// (see `docs/prds/kinematic-constraints.md` task 3 and
    /// `docs/reify-stdlib-reference.md` §13.2). Note: v0.1 detects duplicates by
    /// structural `Value::Eq` rather than by referential identity (the docs spec) —
    /// a code-comment in `mechanism.rs` documents the gap; the docs note will be
    /// updated in the follow-on docs task (2538).
    ///
    /// TODO: wired by the snapshot/eval-pipeline integration in the task family covering
    /// 2585+. The v0.1 mechanism builder records the error condition on the returned
    /// Mechanism `Value::Map` (`error`, `error_message` fields); a follow-on integration
    /// translates the errored Map into a real `Diagnostic` carrying this code via
    /// `EvalResult.diagnostics`. The variant is reserved now so that downstream tooling
    /// (LSP / MCP / IDE error UIs) can match on the typed code identifier from the
    /// moment the diagnostic is emitted, with no further enum churn at integration time.
    MechanismDuplicateSolid,
    /// Origin: `crates/reify-stdlib/src/loop_closure_solver.rs::solve_loop_closure_with_diagnostics`
    /// (task 2677 — PRD `docs/prds/v0_2/kinematic-constraints.md`
    /// §"Singularity, over/under-constraint diagnostics").
    ///
    /// Canonical message form:
    /// `"kinematic singularity detected: rank-deficient Jacobian; last-converged config returned"`.
    ///
    /// Emitted as a `Severity::Warning` when the loop-closure Newton solver
    /// returns [`NewtonOutcome::Singular`](../../reify_stdlib/loop_closure_solver/enum.NewtonOutcome.html#variant.Singular)
    /// (LDLᵀ pivot below `NewtonConfig::singularity_pivot_eps`).
    /// `LoopClosureReport::is_singular()` returns `true` (derived from `outcome`)
    /// and the `Singular` variant's `x` field carries the last-converged config
    /// the PRD requires the snapshot to surface.
    ///
    /// The PRD-prose mnemonic for this code is `W_KINEMATIC_SINGULARITY`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    ///
    /// TODO: surfaced through the snapshot / sweep API in PRD task 10
    /// (snapshot evaluator integration) — `reify-stdlib::snapshot` and the
    /// eval engine do not yet call the wrapper. The variant is reserved now so
    /// downstream tooling (LSP / MCP / IDE error UIs) can match on the typed code
    /// identifier from the moment the diagnostic is first emitted, with no further
    /// enum churn at integration time.
    KinematicSingularity,
    /// Origin: `crates/reify-stdlib/src/loop_closure_solver.rs::solve_loop_closure_with_diagnostics`
    /// (task 2677 — PRD `docs/prds/v0_2/kinematic-constraints.md`
    /// §"Singularity, over/under-constraint diagnostics").
    ///
    /// Canonical message form:
    /// `"kinematic system over-constrained: <N> free DOFs vs 6 loop residuals"`.
    ///
    /// Emitted as a `Severity::Error` when a single-loop closure problem has
    /// fewer free DOFs than the 6-component twist residual (`free_b.len() < 6`).
    /// The wrapper short-circuits the Newton solve and returns
    /// `NewtonOutcome::NotConverged { x, residual_norm: f64::INFINITY }` —
    /// the diagnostic, not a plausible-looking config, is the user-facing
    /// signal of structural infeasibility.
    ///
    /// The PRD-prose mnemonic for this code is `E_KINEMATIC_OVERCONSTRAINED`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    ///
    /// TODO: surfaced through the snapshot / sweep API in PRD task 10
    /// (snapshot evaluator integration). Reserved now for typed-code matching
    /// at the moment the diagnostic is first emitted.
    KinematicOverconstrained,
    /// Origin: `crates/reify-stdlib/src/loop_closure_solver.rs::solve_loop_closure_with_diagnostics`
    /// (task 2677 — PRD `docs/prds/v0_2/kinematic-constraints.md`
    /// §"Singularity, over/under-constraint diagnostics").
    ///
    /// Canonical message form:
    /// `"kinematic system under-constrained: <N> free DOFs vs 6 loop residuals; consider adding an explicit binding"`.
    ///
    /// Emitted as a `Severity::Warning` when a single-loop closure problem has
    /// more free DOFs than the 6-component twist residual (`free_b.len() > 6`).
    /// The Newton solver still runs; the warning suggests an explicit binding.
    /// The "closest-to-previous config" semantics the PRD describes are
    /// realised by the caller's choice of
    /// [`StartStrategy::WarmStart`](../../reify_stdlib/loop_closure_solver/enum.StartStrategy.html#variant.WarmStart),
    /// not by extra logic in the wrapper.
    ///
    /// The PRD-prose mnemonic for this code is `W_KINEMATIC_UNDERCONSTRAINED`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    ///
    /// TODO: surfaced through the snapshot / sweep API in PRD task 10
    /// (snapshot evaluator integration). Reserved now for typed-code matching
    /// at the moment the diagnostic is first emitted.
    KinematicUnderconstrained,
    /// Origin: `crates/reify-eval/src/tolerance_promise.rs::imported_tolerance_promise_diagnostic`
    /// (task 2651 — PRD `docs/prds/v0_2/per-purpose-tolerance.md`
    /// §"Resolved design decisions" → "Imported geometry promise"; arch §10.4 / §14.5).
    ///
    /// Canonical message form:
    /// `"imported geometry '<input_template>' tolerance promise <promise_si>m is insufficient for downstream demand <demanded_si>m; proceeding with as-imported realization"`.
    ///
    /// Emitted as a `Severity::Warning` when the tolerance promise carried by an
    /// `Input` occurrence template (via its `param tolerance : Length = …`
    /// declaration) is strictly looser than the demanded tolerance computed by
    /// `Engine::demanded_tolerance_for_output` (output-bound + active-purpose
    /// combined under "tighter satisfies looser" min-fold). The runtime cannot
    /// verify the imported representation error for arbitrary STEP/STL input,
    /// so the contract is a *promise*: the runtime emits a warning (not an
    /// error) and proceeds with the as-imported realization. Users opt into
    /// explicit re-meshing/healing through a stdlib helper rather than the
    /// runtime silently doing it.
    ///
    /// The PRD-prose mnemonic for this code is `W_IMPORTED_TOLERANCE_INSUFFICIENT`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Mirrors the
    /// advisory-warning posture established by `FieldOutOfBounds`,
    /// `TraitUserAsserted`, and `TopologyTagStale`: downstream tooling that
    /// wants to surface these as harder failures can filter by code at the
    /// consumer side.
    ImportedTolerancePromiseInsufficient,
    /// Origin: `crates/reify-eval/src/tolerance_promise.rs::input_tolerance_promise_is_zero_diagnostic`
    /// (task 2833 — PRD `docs/prds/v0_2/per-purpose-tolerance.md`
    /// §"Resolved design decisions" → "Imported geometry promise").
    ///
    /// Canonical message form:
    /// `"imported geometry '<input_template>' carries a zero tolerance promise \
    /// (`tolerance = 0m`) but downstream demand is <demanded_str>; the zero promise \
    /// vacuously satisfies any non-negative demand, suppressing the \
    /// ImportedTolerancePromiseInsufficient warning. Omit the `tolerance` parameter \
    /// to opt out of making a promise."`.
    ///
    /// Emitted as a `Severity::Warning` by `Engine::check_imported_tolerance_promise`
    /// when the imported-geometry tolerance promise carried by an `Input` occurrence
    /// template is **exactly `0.0`** AND the demanded tolerance is **strictly positive**
    /// (`demanded > 0.0`). This surfaces the placeholder-default footgun where
    /// `param tolerance : Length = 0m` would otherwise silently disable the
    /// `ImportedTolerancePromiseInsufficient` warning via the strict-`<` rule
    /// (when `promise == 0.0`, `demanded < 0.0` is false for every `demanded >= 0.0`,
    /// so the insufficient branch never fires).
    ///
    /// The PRD-prose mnemonic for this code is `W_INPUT_TOLERANCE_PROMISE_IS_ZERO`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Mirrors the advisory-warning
    /// posture of `ImportedTolerancePromiseInsufficient` and `FieldOutOfBounds`:
    /// the realization proceeds; the warning gives the author visibility so they can
    /// either remove the `tolerance` parameter (the recommended opt-out — omitting
    /// it causes `extract_input_tolerance_promise` to return `None` via Gate 1, the
    /// same path as a missing binding) or replace `0m` with the true measured tolerance.
    InputTolerancePromiseIsZero,
    /// Origin: `crates/reify-eval/src/dispatcher.rs::long_chain_diagnostic`
    /// (task 2646 — PRDs `docs/prds/v0_2/multi-kernel.md`
    /// §"Resolved design decisions" → "Long-chain diagnostic" and
    /// `docs/prds/v0_2/per-purpose-tolerance.md`
    /// §"Resolved design decisions" → "Long-chain diagnostic gating").
    ///
    /// Canonical message form:
    /// `"long-chain realization (<N> stages, elapsed <ms>ms > <threshold_ms>ms): \
    /// <kernel_a>: <from>→<to> → <kernel_b>: <from>→<to> → … → <final_kernel>"`.
    ///
    /// Emitted as a `Severity::Warning` when the dispatcher selects a chain
    /// **longer than 2 conversion stages** (strict `>` 2 ⇒ ≥3 stages) AND
    /// elapsed realization wall time **exceeds the configured threshold**
    /// (strict `>`; default 500 ms, override via the `REIFY_LONG_CHAIN_THRESHOLD_MS`
    /// environment variable). Both gates must hold; short-chain pain is
    /// self-evident and a sub-threshold long chain is not user-visible budget
    /// pressure, so suppressing those cases is intentional ergonomics.
    ///
    /// The diagnostic NAMES THE CHAIN — each conversion stage's kernel and
    /// `from→to` repr transition, plus the final-stage kernel — so users can
    /// see exactly where the conversion budget is going (PRD: "names the
    /// chain so users can see budget pressure"). Strict-`>` gating mirrors
    /// the canonical decision in
    /// `reify_eval::tolerance_promise::is_promise_insufficient`
    /// (task 2651): boundary cases (exactly 2 stages, exactly 500 ms) do
    /// NOT warn — consistent with the "tighter satisfies looser" partial-order
    /// vocabulary throughout the tolerance subsystem. The link is rendered
    /// as plain code-formatted prose (not an intra-doc link) because
    /// `reify-types` is a *dependency* of `reify-eval`, not vice-versa, so
    /// rustdoc cannot resolve a real link in this direction.
    ///
    /// The PRD-prose mnemonic for this code is `W_LONG_CHAIN_REALIZATION`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Mirrors the
    /// advisory-warning posture established by
    /// `ImportedTolerancePromiseInsufficient`, `FieldOutOfBounds`, and
    /// `KinematicSingularity`: the realization completed; the user just
    /// deserves visibility into budget pressure. Downstream tooling that
    /// wants to surface this as a harder failure (e.g. CI gate) can filter
    /// by code at the consumer side.
    LongChainRealization,
    /// Origin: `crates/reify-eval/src/dispatcher.rs::no_kernel_chain_diagnostic`
    /// (task 3434 — PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task γ +
    /// §2 "failing closed is the failure mode").
    ///
    /// Canonical message form:
    /// `"no kernel chain found for op '<Operation:?>' to produce '<ReprKind:?>'; \
    /// available reprs: [<ReprKind:?>, ...]"`.
    ///
    /// Emitted as a `Severity::Error` when the multi-kernel dispatcher's BFS
    /// over reachable [`ReprKind`](super::ReprKind) states exhausts without
    /// reaching the demanded repr (or no registered kernel claims `(op,
    /// demanded)` in its supports table). Mirrors PRD §2: the dispatcher
    /// fails closed rather than silently picking an incompatible kernel —
    /// the user gets a typed error and can adjust their kernel set or
    /// `#kernel(...)` pragma. Available reprs are rendered from a
    /// [`BTreeSet`](std::collections::BTreeSet) for deterministic ordering
    /// across runs (the underlying `HashSet<ReprKind>` iteration is
    /// hash-seeded; see `dispatch_seeding_order_is_deterministic` at
    /// `dispatcher.rs:1010-1080`).
    ///
    /// The PRD-prose mnemonic for this code is `E_NO_KERNEL_CHAIN`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Consumed by
    /// downstream tasks δ/ε (IDs 3435/3436) which wire the dispatcher None-
    /// return into op-execution; until then this is scaffolding alongside
    /// `LongChainRealization`'s established precedent (task 2646).
    NoKernelChain,
    /// Origin: `crates/reify-eval/src/dispatcher.rs::kernel_pragma_unsatisfiable_diagnostic`
    /// (task 3434 — PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task γ +
    /// §5 "pragma steers").
    ///
    /// Canonical message form:
    /// `"#kernel('<pragma_kernel>') cannot serve op '<Operation:?>' producing \
    /// '<ReprKind:?>'; falling through to default kernel selection"`.
    ///
    /// Emitted as a `Severity::Warning` when a `#kernel(...)` pragma names
    /// a kernel that does not support the demanded `(op, demanded)` pair.
    /// Per PRD §5: "warning, not error — fall through to default lex-min
    /// selection so the user's design still evaluates" — the realization
    /// proceeds via the default selection path; the warning gives the author
    /// visibility into the unmet preference. Mirrors the advisory-warning
    /// posture established by `LongChainRealization` and
    /// `ImportedTolerancePromiseInsufficient`.
    ///
    /// The PRD-prose mnemonic for this code is `W_KERNEL_PRAGMA_UNSATISFIABLE`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Consumed by
    /// downstream task ο (ID 3443) which wires the `#kernel(...)` pragma
    /// surface into the dispatcher's preference path.
    KernelPragmaUnsatisfiable,
    /// Origin: `crates/reify-eval/src/dispatcher.rs::pinned_kernel_missing_diagnostic`
    /// (task 3434 — PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task γ +
    /// §5 "Pin name not in registry").
    ///
    /// Canonical message form:
    /// `"kernel '<kernel_id>' is pinned in reify.toml but not registered in \
    /// this build; rebuild with the required kernel feature enabled"`.
    ///
    /// Emitted as a `Severity::Error` when `reify.toml` `[kernels]` names a
    /// kernel that the current build did not register (typically because the
    /// corresponding Cargo feature was not enabled). Per PRD §5: "error;
    /// engine refuses to start" — the build's determinism contract requires
    /// every pinned kernel to be present, so the engine fails closed at
    /// startup rather than silently downgrading to a different kernel set.
    ///
    /// The PRD-prose mnemonic for this code is `E_PINNED_KERNEL_MISSING`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Consumed by
    /// downstream task π (ID 3444) which wires `reify.toml` parsing into
    /// `Engine::with_registered_kernels`.
    PinnedKernelMissing,
    /// Origin: `crates/reify-eval/src/dispatcher.rs::unpinned_kernel_loaded_diagnostic`
    /// (task 3434 — PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task γ +
    /// §5 "Registry name not pinned").
    ///
    /// Canonical message form:
    /// `"kernel '<kernel_id>' is registered but not listed in reify.toml \
    /// [kernels]; consider pinning it for build determinism"`.
    ///
    /// Emitted as a `Severity::Warning` when a kernel is present in the
    /// registry but not listed in `reify.toml` `[kernels]`. Per PRD §5:
    /// "warning; engine starts" — the realization proceeds (the kernel is
    /// usable), but the missing pin weakens the determinism contract: a
    /// future build that omits the same kernel feature could shift kernel
    /// selection unexpectedly. Mirrors the advisory-warning posture of
    /// `LongChainRealization` and `ImportedTolerancePromiseInsufficient`.
    ///
    /// The PRD-prose mnemonic for this code is `W_UNPINNED_KERNEL_LOADED`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Consumed by
    /// downstream task π (ID 3444) which wires `reify.toml` parsing into
    /// `Engine::with_registered_kernels`.
    UnpinnedKernelLoaded,
    /// Origin: `crates/reify-eval/src/dispatcher.rs::kernel_version_mismatch_diagnostic`
    /// (task 3434 — PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task γ +
    /// §5 "Pin version mismatch with adapter VERSION constant").
    ///
    /// Canonical message form:
    /// `"kernel '<kernel_id>' version mismatch: reify.toml pins '<pinned>' \
    /// but adapter VERSION = '<actual>'; determinism contract requires \
    /// matching versions"`.
    ///
    /// Emitted as a `Severity::Error` when `reify.toml` pins a kernel
    /// version that disagrees with the adapter's compiled-in `VERSION`
    /// constant. Per PRD §5: "error. Determinism contract enforcement" —
    /// matching versions is load-bearing for reproducible realization
    /// across build hosts; the engine fails closed rather than silently
    /// using a different adapter than the project pins.
    ///
    /// The PRD-prose mnemonic for this code is `E_KERNEL_VERSION_MISMATCH`
    /// (severity convention: `W_*` → Warning, `E_*` → Error). Consumed by
    /// downstream task π (ID 3444) which wires `reify.toml` parsing into
    /// `Engine::with_registered_kernels`.
    KernelVersionMismatch,
    /// Origin: `crates/reify-eval/src/geometry_ops.rs::gate_query_capability`
    /// (task 3623 — PRD `docs/prds/v0_3/kernel-geometry-queries.md` §5.4).
    ///
    /// Canonical message form (the 'requires' clause is capability-dependent):
    /// - `BRepOnly` query: `"'<helper>' requires BRep representation; this geometry is realized as <Repr>"`
    /// - `MeshOnly` query: `"'<helper>' requires Mesh representation; this geometry is realized as <Repr>"`
    /// - `BRepAndMesh` query: `"'<helper>' requires BRep or Mesh representation; this geometry is realized as <Repr>"`
    ///
    /// Emitted as a `Severity::Error` by `gate_query_capability` when a query
    /// is dispatched against an unsupported realization
    /// (`ReprKind::Mesh`/`Sdf`/`Voxel`/`VolumeMesh`). The gate fails closed:
    /// the caller maps `CapabilityRoute::Unsupported` → `None` → the cell
    /// retains `Value::Undef` (the existing fall-through-is-preservation
    /// contract). The helper name (`<helper>`) is the user-written `.ri`
    /// function name (e.g. `"curvature"`, `"edge_length"`); the repr token
    /// is the `Debug` representation of `ReprKind` (e.g. `"Mesh"`, `"Voxel"`).
    ///
    /// The PRD-prose mnemonic for this code is `E_QUERY_NOT_SUPPORTED_ON_REPR`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    QueryNotSupportedOnRepr,
    /// A declared type name failed to resolve in any compile-time context.
    ///
    /// Origin sites (all carry this code):
    /// - `crates/reify-compiler/src/functions.rs:34` — function parameter type (KEY site)
    /// - `crates/reify-compiler/src/functions.rs:122` — function return type
    /// - `crates/reify-compiler/src/functions.rs:280,290,301` — field domain type
    ///   (DimensionalOp / IntegerLiteral / Auto arms)
    /// - `crates/reify-compiler/src/functions.rs:319,333,347` — field codomain type
    ///   (DimensionalOp / IntegerLiteral / Auto arms)
    /// - `crates/reify-compiler/src/guards.rs:155` — purpose-guard parameter type
    /// - `crates/reify-compiler/src/entity.rs:487` — entity-member parameter type
    /// - `crates/reify-compiler/src/entity.rs:742-743` — port parameter type
    /// - `crates/reify-compiler/src/expr.rs:2294-2300` — lambda parameter type (Named arm)
    /// - `crates/reify-compiler/src/expr.rs:2305-2311` — lambda parameter type (non-Named arm)
    /// - `crates/reify-compiler/src/traits.rs:34-42` — trait member type (DimensionalOp)
    /// - `crates/reify-compiler/src/traits.rs:87-92` — trait member type (resolve-fail)
    /// - `crates/reify-compiler/src/conformance/checker.rs:132-138` — conformance type (DimensionalOp)
    /// - `crates/reify-compiler/src/conformance/checker.rs:185-188` — conformance type (resolve-fail)
    /// - `crates/reify-compiler/src/type_resolution.rs:1015-1021` — type-alias argument
    ///
    /// Canonical message forms (context prefix only annotates the declaration site;
    /// the root semantic — a declared type name failed to resolve — is identical
    /// across all forms, so they share one code rather than per-context codes):
    /// - `"unresolved type: <name>"` (bare form)
    /// - `"unresolved return type: <name>"`
    /// - `"unresolved field type: <expr>"`
    /// - `"unresolved type in lambda param '<p>': <name>"`
    /// - `"unresolved type in trait '<t>': <name>"`
    /// - `"unresolved type in conformance check: <name>"`
    /// - `"unresolved type argument '<arg>' for alias '<alias>'"`
    /// - `"unresolved type name '<n>' in port parameter"`
    ///
    /// The PRD-prose mnemonic for this code is `E_UNRESOLVED_TYPE`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    UnresolvedType,
    /// An expression references an unbound identifier at compile time.
    ///
    /// Origin sites (all carry this code):
    /// - `crates/reify-compiler/src/expr.rs:670-681` — unbound identifier in expression
    ///   context (KEY site; also emits the `"did you mean \`<canonical>\`?"` hint variant)
    /// - `crates/reify-compiler/src/annotations.rs:321` — solver-hint collection reference
    ///   (relocated from old line 500 in a file reorganisation)
    ///
    /// Canonical message forms:
    /// - `"unresolved name: <name>"`
    /// - `"unresolved name: <name> (did you mean \`<canonical>\`?)"` (builtin-hint variant)
    ///
    /// The PRD-prose mnemonic for this code is `E_UNRESOLVED_NAME`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    UnresolvedName,
    /// Origin: `crates/reify-eval/src/shell_extract_compute.rs` (γ trampoline
    /// mapping of [`reify_shell_extract::SegmentationError::InvalidThreshold`]).
    ///
    /// Emitted as `Severity::Error` when the `shell_threshold` option supplied
    /// to `"shell-extract::extract"` is ≤ 0 or ≥ 1. The producer's
    /// `segment_regions` function requires `shell_threshold ∈ (0.0, 1.0)`.
    ///
    /// Canonical message form:
    /// `"shell_threshold = <value> must be in (0.0, 1.0)."`
    ///
    /// Introduced in task γ (#3834, `shell-extract-engine-bridge.md` §7 row 3).
    /// The remaining six PRD §7 codes (`ShellNoVoxelGrid`, `ShellMedialMaskOob`,
    /// `ShellPruneFailed`, `ShellMeshQuality`, `ShellTooThick`, `ShellNoMedial`)
    /// are deferred to task ε; only this code is wired in γ because it is the
    /// only user-facing variant the γ test exercises.
    ///
    /// The PRD-prose mnemonic for this code is `E_SHELL_BAD_THRESHOLD`.
    ShellBadThreshold,
    /// Origin: `crates/reify-stdlib/src/stackup.rs` (classifier) +
    ///          `crates/reify-expr/src/lib.rs` (emission site).
    ///
    /// Emitted as `Severity::Error` when `stackup_worst_case`, `stackup_rss`,
    /// or `monte_carlo_stackup` receives an empty list (`[]`) as the chain
    /// argument.  An empty chain yields no contributors and no meaningful gap
    /// statistics.
    ///
    /// Canonical message form:
    /// `"E_StackupEmptyChain: tolerance chain must be non-empty"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_StackupEmptyChain`
    /// (see `docs/prds/v0_6/tolerance-stackup-analysis.md` §4.4).
    StackupEmptyChain,
    /// Origin: `crates/reify-stdlib/src/stackup.rs` (classifier) +
    ///          `crates/reify-expr/src/lib.rs` (emission site).
    ///
    /// Emitted as `Severity::Error` when a contributor entry in the chain
    /// is not a `Value::Map`, or when the `nominal`, `plus_tol`, or
    /// `minus_tol` field of a contributor map is not a finite LENGTH scalar.
    ///
    /// Canonical message form:
    /// `"E_StackupDimMismatch: contributor field must be a finite LENGTH scalar"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_StackupDimMismatch`
    /// (see `docs/prds/v0_6/tolerance-stackup-analysis.md` §4.4).
    StackupDimMismatch,
    /// Origin: `crates/reify-stdlib/src/stackup.rs` (classifier) +
    ///          `crates/reify-expr/src/lib.rs` (emission site).
    ///
    /// Emitted as `Severity::Error` when the `sign` field of a contributor
    /// map is not `Value::Int(1)` or `Value::Int(-1)`.
    ///
    /// Canonical message form:
    /// `"E_StackupBadSign: contributor sign must be Int(+1) or Int(-1)"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_StackupBadSign`
    /// (see `docs/prds/v0_6/tolerance-stackup-analysis.md` §4.4).
    StackupBadSign,
    /// Origin: `crates/reify-stdlib/src/stackup.rs` (classifier) +
    ///          `crates/reify-expr/src/lib.rs` (emission site).
    ///
    /// Emitted as `Severity::Error` when the `samples` argument to
    /// `monte_carlo_stackup` is not a positive `Value::Int` (i.e. ≤ 0
    /// or not an integer type).
    ///
    /// Canonical message form:
    /// `"E_StackupBadSamples: samples must be a positive integer"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_StackupBadSamples`
    /// (see `docs/prds/v0_6/tolerance-stackup-analysis.md` §4.4).
    StackupBadSamples,
    /// Origin: `crates/reify-compiler/src/entity.rs` (main sub-lowering arm).
    ///
    /// Canonical message form:
    /// `"'at' placement is not supported on collection subs; per-element placement is out of scope in v1"`.
    ///
    /// Emitted as `Severity::Error` when a `sub` declaration marked as a
    /// collection (`sub name : List<T>`) also carries an `at <pose>` clause.
    /// Per PRD §10 and the AST doc-comment on `SubDecl.pose_expr`, per-element
    /// placement of collection subs requires per-instance realization handles
    /// deferred to spec §8.3; the grammar admits the syntax but the compiler
    /// rejects the combination semantically.
    ///
    /// The `at` clause's span is attached as a primary label
    /// (`"'at' not allowed on collection sub"`). The invalid pose expression
    /// is discarded (`SubComponentDecl.pose` is set to `None`); `aux` on the
    /// same collection sub remains valid and is lowered normally.
    ///
    /// The PRD-prose mnemonic for this code is `E_AT_ON_COLLECTION_SUB`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    AtOnCollectionSub,
}

/// A diagnostic message with location and optional labels.
///
/// # Construction
///
/// Use [`Diagnostic::error`], [`Diagnostic::warning`], or [`Diagnostic::info`] to
/// create a diagnostic, then chain [`Diagnostic::with_code`], [`Diagnostic::with_label`],
/// and/or [`Diagnostic::with_candidates`] as needed.
///
/// Direct struct-literal construction is not supported for external crates:
///
/// ```compile_fail,E0639
/// use reify_core::{Diagnostic, Severity};
/// let _ = Diagnostic {
///     severity: Severity::Error,
///     message: String::new(),
///     labels: vec![],
///     code: None,
///     candidates: vec![],
/// };
/// ```
///
/// **Note on the doctest above:** the `compile_fail,E0639` annotation documents the
/// expected error code but rustdoc does not validate it — the test only asserts that
/// the snippet fails to compile, not *why* it fails. The real enforcement comes from
/// `#[non_exhaustive]` itself: if the attribute is ever removed, this snippet would
/// compile successfully and the test would turn red, reliably signalling the regression.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<DiagnosticLabel>,
    /// Typed kind of this diagnostic. `None` for legacy emissions that have
    /// not yet been migrated. Producers attach a code via [`Diagnostic::with_code`].
    pub code: Option<DiagnosticCode>,
    /// Machine-readable candidate set for "expected one of …" diagnostics.
    /// Empty for diagnostics that do not enumerate alternatives.
    /// Producers attach via [`Diagnostic::with_candidates`]; consumers
    /// (LSP quick-fixes, IDE error UIs) may read this without parsing the
    /// human-readable message.
    ///
    /// # Invariant: bare FQN entries only
    ///
    /// Each entry is a single bare FQN (e.g. `"foo::ORingSeal"`), NEVER a
    /// composite `name=value,name=value` tuple. Multi-valued or structured
    /// witness summaries (e.g. cross-product witnesses) belong in the
    /// human-readable [`Diagnostic::message`] field; producers that need
    /// to surface a multi-dimensional witness must collapse to the
    /// lex-first leaf's FQNs (or the flat FQN union) before calling
    /// [`Diagnostic::with_candidates`]. Downstream consumers in
    /// `crates/reify-lsp/src/convert.rs` flatten this list verbatim into
    /// the LSP `data` JSON `{"candidates": [...]}` — a quick-fix provider
    /// that splits entries on the FQN convention will silently mis-parse
    /// joined labels. See task 2860 for the contract origin.
    ///
    /// # Single-param vs. multi-param interpretation
    ///
    /// The *shape* of this list differs by emission context:
    ///
    /// - **Single-param "pick one" sites** (e.g. `AutoTypeParamPoolOverflow`,
    ///   `AutoTypeParamNoCandidate`) pack multiple alternative FQNs — a
    ///   consumer should offer each as an independent substitution choice.
    /// - **Multi-param "coherent assignment" sites** (e.g.
    ///   `AutoTypeParamAmbiguous` when ≥2 cross-product assignments exist)
    ///   pack the FQNs of a *single coherent assignment* — one FQN per
    ///   declared parameter in declared order. The entries must be applied
    ///   *together*, not as independent alternatives.
    ///
    /// Consumers that need to distinguish these two shapes must inspect the
    /// [`Diagnostic::code`] field (e.g. `AutoTypeParamAmbiguous` signals the
    /// multi-param case). Treating a multi-param "all-of-these-together" list
    /// as a "pick one" list will produce incoherent quick-fixes. Task 2663
    /// (search-failure diagnostic format) extended this contract to
    /// `AutoTypeParamNoCandidate`'s v0.2 cross-product `0 =>` arm — see that
    /// code's doc-comment for the multi-param prefix-illustration shape. The
    /// FQN-only invariant above is preserved across both multi-param sites;
    /// any future richer structured representation should layer on rather
    /// than violate it.
    pub candidates: Vec<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            labels: Vec::new(),
            code: None,
            candidates: Vec::new(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            labels: Vec::new(),
            code: None,
            candidates: Vec::new(),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Info,
            message: message.into(),
            labels: Vec::new(),
            code: None,
            candidates: Vec::new(),
        }
    }

    pub fn with_label(mut self, label: DiagnosticLabel) -> Self {
        self.labels.push(label);
        self
    }

    /// Attach a typed [`DiagnosticCode`] to this diagnostic.
    ///
    /// Mirrors [`Diagnostic::with_label`]: builder-fluent, takes ownership,
    /// returns `Self`. Callers chain `.with_code(DiagnosticCode::X)` between
    /// `Diagnostic::error(...)` and `.with_label(...)`.
    pub fn with_code(mut self, code: DiagnosticCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Attach a machine-readable candidate list to this diagnostic.
    ///
    /// Mirrors [`Diagnostic::with_code`]: builder-fluent, takes ownership,
    /// returns `Self`. Callers chain `.with_candidates(items)` to expose
    /// the "expected one of …" set as a structured field so downstream
    /// consumers (LSP quick-fixes, IDE error UIs) can read it without
    /// parsing the human-readable message.
    ///
    /// Accepts any `IntoIterator` whose items convert to `String`, so
    /// callers can pass `&[&str]`, an iterator of `&str`, or a
    /// pre-built `Vec<String>` without an intermediate allocation.
    pub fn with_candidates<I, S>(mut self, candidates: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.candidates = candidates.into_iter().map(Into::into).collect();
        self
    }
}

/// A label pointing to a specific location in source code.
#[derive(Debug, Clone)]
pub struct DiagnosticLabel {
    pub span: SourceSpan,
    pub message: String,
}

impl DiagnosticLabel {
    pub fn new(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

/// A lightweight reference to a diagnostic (for constraint results etc.)
#[derive(Debug, Clone)]
pub struct DiagnosticRef {
    pub index: usize,
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticCode, SourceSpan};

    #[test]
    fn prelude_sentinel_is_prelude() {
        assert!(
            SourceSpan::prelude().is_prelude(),
            "SourceSpan::prelude() must satisfy is_prelude()"
        );
    }

    #[test]
    fn empty_zero_is_not_prelude() {
        assert!(
            !SourceSpan::empty(0).is_prelude(),
            "SourceSpan::empty(0) must NOT satisfy is_prelude()"
        );
    }

    #[test]
    fn regular_span_is_not_prelude() {
        assert!(
            !SourceSpan::new(0, 5).is_prelude(),
            "SourceSpan::new(0, 5) must NOT satisfy is_prelude()"
        );
    }

    #[test]
    fn prelude_distinct_from_empty_zero() {
        assert_ne!(
            SourceSpan::prelude(),
            SourceSpan::empty(0),
            "SourceSpan::prelude() must be distinct from SourceSpan::empty(0)"
        );
    }

    /// `Diagnostic::error` defaults `code` to `None` — opt-in via `with_code` only.
    #[test]
    fn diagnostic_default_code_is_none() {
        let d = Diagnostic::error("x");
        assert_eq!(d.code, None);
    }

    /// `with_code` attaches the supplied `DiagnosticCode` and is fluent (returns `Self`).
    #[test]
    fn diagnostic_with_code_attaches_code() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::TraitNotImplemented);
        assert_eq!(d.code, Some(DiagnosticCode::TraitNotImplemented));
    }

    /// `Diagnostic::error` defaults `candidates` to empty — opt-in via `with_candidates` only.
    #[test]
    fn diagnostic_default_candidates_is_empty() {
        let d = Diagnostic::error("x");
        assert_eq!(d.candidates, Vec::<String>::new());
    }

    /// `with_candidates` attaches the supplied candidate list and is fluent (returns `Self`).
    /// Verify that it chains with other builder methods.
    #[test]
    fn diagnostic_with_candidates_attaches_candidates() {
        let d = Diagnostic::error("x").with_candidates(vec!["A".to_string(), "B".to_string()]);
        assert_eq!(d.candidates, vec!["A".to_string(), "B".to_string()]);
        // Fluency check: with_candidates composes with with_code and with_label
        use super::DiagnosticLabel;
        use super::SourceSpan;
        let d2 = Diagnostic::error("y")
            .with_code(DiagnosticCode::TraitNotImplemented)
            .with_candidates(vec!["X".to_string()])
            .with_label(DiagnosticLabel::new(SourceSpan::prelude(), "lbl"));
        assert_eq!(d2.candidates, vec!["X".to_string()]);
        assert_eq!(d2.code, Some(DiagnosticCode::TraitNotImplemented));
        assert_eq!(d2.labels.len(), 1);
    }

    /// `DiagnosticCode` is `Copy + Clone + PartialEq + Eq + Hash + Debug`.
    /// (Compile-tested by exercising each of those bounds in the body.)
    #[test]
    fn diagnostic_code_derives() {
        use std::collections::HashSet;
        let a = DiagnosticCode::TraitNotImplemented;
        let b: DiagnosticCode = a; // Copy
        let c = a; // Copy again — `a` still usable below
        assert_eq!(a, b); // PartialEq
        assert_eq!(a, c); // PartialEq
        let _: DiagnosticCode = Clone::clone(&a); // Clone (explicit to bypass clippy::clone_on_copy)
        let mut set: HashSet<DiagnosticCode> = HashSet::new();
        assert!(set.insert(a)); // Hash + Eq
        assert!(!set.insert(b)); // dedup on Eq
        let _ = format!("{:?}", a); // Debug
    }

    /// Under `feature = "serde"`, `DiagnosticCode` serializes to its PascalCase variant name.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TraitNotImplemented).unwrap();
        assert_eq!(s, "\"TraitNotImplemented\"");
    }

    /// `DiagnosticCode::DeepDotChain` is a real variant: it constructs, supports
    /// equality (mirrors `diagnostic_code_derives`), and Debug-prints as `"DeepDotChain"`.
    /// Pairs with the lint pass in
    /// `crates/reify-compiler/src/compile_builder/dot_chain_lint.rs`.
    #[test]
    fn diagnostic_code_deep_dot_chain_variant() {
        let a = DiagnosticCode::DeepDotChain;
        let b = a; // Copy
        assert_eq!(a, b); // PartialEq + Eq
        assert_eq!(format!("{:?}", a), "DeepDotChain");
    }

    // --- DimensionMismatch tests (step-3) ---
    // Note: Copy/Clone/PartialEq/Eq/Hash/Debug derives for DimensionMismatch are
    // already covered by the variant-agnostic `diagnostic_code_derives` test above.
    // Only the serde wire-format test is kept here because it is genuinely
    // variant-specific (PascalCase serialization of the exact string "DimensionMismatch").

    /// Under `feature = "serde"`, `DiagnosticCode::DimensionMismatch` serializes as
    /// `"DimensionMismatch"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_dimension_mismatch_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::DimensionMismatch).unwrap();
        assert_eq!(s, "\"DimensionMismatch\"");
    }

    // --- GeometryUnbounded tests (geometry-traits task 2312) ---
    // Pairs with the conformance-walker producer in
    // `crates/reify-compiler/src/conformance/mod.rs` for the call-site
    // Bounded check at trait-typed parameters of `Type::Geometry` arguments.

    /// `DiagnosticCode::GeometryUnbounded` round-trips through
    /// `Diagnostic::error(...).with_code(...)` (mirrors the variant-agnostic
    /// `diagnostic_code_derives` shape but targeted at the new variant so a
    /// future enum reorganisation that drops it is caught here).
    #[test]
    fn diagnostic_code_geometry_unbounded_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::GeometryUnbounded);
        assert_eq!(d.code, Some(DiagnosticCode::GeometryUnbounded));
        assert_eq!(
            format!("{:?}", DiagnosticCode::GeometryUnbounded),
            "GeometryUnbounded"
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::GeometryUnbounded` serializes as
    /// `"GeometryUnbounded"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_geometry_unbounded_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::GeometryUnbounded).unwrap();
        assert_eq!(s, "\"GeometryUnbounded\"");
    }

    // --- Shadowing tests (task 2310 — spec §8.5) ---
    // Pairs with the lint pass in
    // `crates/reify-compiler/src/compile_builder/shadow_lint.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::Shadowing` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` and Debug-prints as `"Shadowing"`.
    /// Shape mirrors `diagnostic_code_geometry_unbounded_with_code_round_trips`
    /// (which targets a different variant); a future enum reorganisation that
    /// drops `Shadowing` is caught here.
    #[test]
    fn diagnostic_code_shadowing_with_code_round_trips() {
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::Shadowing);
        assert_eq!(d.code, Some(DiagnosticCode::Shadowing));
        assert_eq!(format!("{:?}", DiagnosticCode::Shadowing), "Shadowing");
    }

    /// Under `feature = "serde"`, `DiagnosticCode::Shadowing` serializes as
    /// `"Shadowing"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_shadowing_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::Shadowing).unwrap();
        assert_eq!(s, "\"Shadowing\"");
    }

    // --- TraitUserAsserted tests (task 2321 — W_TRAIT_USER_ASSERTED) ---
    // Pairs with the per-bound lint in `crates/reify-compiler/src/entity.rs`
    // (trait_bound iteration). Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug
    // derives are already covered by `diagnostic_code_derives` above; only the
    // variant-specific round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::TraitUserAsserted` round-trips through
    /// `Diagnostic::warning(...).with_code(...)`.  Shape mirrors
    /// `diagnostic_code_shadowing_with_code_round_trips`; a future enum
    /// reorganisation that drops `TraitUserAsserted` is caught here.
    /// (The `Debug` rendering assertion is omitted — it would only pin the
    /// identifier spelling, which any rename touches on both sides simultaneously.
    /// The serde wire-format test below provides the real external-contract pin.)
    #[test]
    fn diagnostic_code_trait_user_asserted_with_code_round_trips() {
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::TraitUserAsserted);
        assert_eq!(d.code, Some(DiagnosticCode::TraitUserAsserted));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TraitUserAsserted` serializes as
    /// `"TraitUserAsserted"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_trait_user_asserted_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TraitUserAsserted).unwrap();
        assert_eq!(s, "\"TraitUserAsserted\"");
    }

    // --- TopologyTagStale tests (task 2332 — W_TOPOLOGY_TAG_STALE) ---
    // Pairs with the resolver `resolve_unique_by_tag` in
    // `crates/reify-eval/src/topology_selectors.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::TopologyTagStale` round-trips through
    /// `Diagnostic::warning(...).with_code(...)`.
    /// Shape mirrors `diagnostic_code_trait_user_asserted_with_code_round_trips`; a future
    /// enum reorganisation that drops `TopologyTagStale` is caught here.
    /// (The `Debug` rendering assertion is omitted — it would only pin the
    /// identifier spelling, which any rename touches on both sides simultaneously.
    /// The serde wire-format test below provides the real external-contract pin.)
    #[test]
    fn diagnostic_code_topology_tag_stale_with_code_round_trips() {
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::TopologyTagStale);
        assert_eq!(d.code, Some(DiagnosticCode::TopologyTagStale));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TopologyTagStale` serializes as
    /// `"TopologyTagStale"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_topology_tag_stale_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TopologyTagStale).unwrap();
        assert_eq!(s, "\"TopologyTagStale\"");
    }

    // --- SpecializationForbiddenDecl tests (task 2369 — E_SPECIALIZATION_FORBIDDEN_DECL) ---
    // Pairs with the rejection rule in
    // `crates/reify-compiler/src/compile_builder/specialization_scope_check.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::SpecializationForbiddenDecl` round-trips through
    /// `Diagnostic::error(...).with_code(...)` and reports
    /// `Some(DiagnosticCode::SpecializationForbiddenDecl)`.
    /// Shape mirrors `diagnostic_code_topology_tag_stale_with_code_round_trips`;
    /// a future enum reorganisation that drops `SpecializationForbiddenDecl` is
    /// caught here.
    #[test]
    fn diagnostic_code_specialization_forbidden_decl_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::SpecializationForbiddenDecl);
        assert_eq!(d.code, Some(DiagnosticCode::SpecializationForbiddenDecl));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::SpecializationForbiddenDecl` serializes as
    /// `"SpecializationForbiddenDecl"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_specialization_forbidden_decl_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::SpecializationForbiddenDecl).unwrap();
        assert_eq!(s, "\"SpecializationForbiddenDecl\"");
    }

    // --- PurposeLetUnsupported tests (task 2537 — E_PURPOSE_LET_UNSUPPORTED) ---
    // Pairs with the unsupported-feature error in
    // `crates/reify-compiler/src/traits.rs::compile_purpose` (Let arm).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::PurposeLetUnsupported` round-trips through
    /// `Diagnostic::error(...).with_code(...)` and reports
    /// `Some(DiagnosticCode::PurposeLetUnsupported)`.
    /// Shape mirrors `diagnostic_code_specialization_forbidden_decl_with_code_round_trips`;
    /// a future enum reorganisation that drops `PurposeLetUnsupported` is caught here.
    /// (The `Debug` rendering assertion is omitted — it would only pin the identifier
    /// spelling, which any rename touches on both sides simultaneously. The serde
    /// wire-format test below provides the real external-contract pin.)
    #[test]
    fn diagnostic_code_purpose_let_unsupported_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::PurposeLetUnsupported);
        assert_eq!(d.code, Some(DiagnosticCode::PurposeLetUnsupported));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::PurposeLetUnsupported` serializes as
    /// `"PurposeLetUnsupported"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_purpose_let_unsupported_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::PurposeLetUnsupported).unwrap();
        assert_eq!(s, "\"PurposeLetUnsupported\"");
    }

    // --- FieldOutOfBounds tests (task 2341 — W_FIELD_OUT_OF_BOUNDS) ---
    // Pairs with the runtime out-of-bounds detector in
    // `crates/reify-expr/src/sampled.rs::sample_at_point`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and severity tests are added here.

    /// `DiagnosticCode::FieldOutOfBounds` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::FieldOutOfBounds)`.
    /// Pins existence of the new variant for v0.2 sampled-field OOB detection.
    #[test]
    fn field_out_of_bounds_diagnostic_code_is_constructible() {
        use super::Severity;
        let d = Diagnostic::warning("oob").with_code(DiagnosticCode::FieldOutOfBounds);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::FieldOutOfBounds));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::FieldOutOfBounds` serializes as
    /// `"FieldOutOfBounds"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_field_out_of_bounds_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::FieldOutOfBounds).unwrap();
        assert_eq!(s, "\"FieldOutOfBounds\"");
    }

    // --- FieldSampledInvalidConfig tests (task 2341 — W_FIELD_SAMPLED_INVALID_CONFIG) ---
    // Pairs with the runtime parse-failure / invariant-violation handler in
    // `crates/reify-eval/src/engine_eval.rs::build_sampled_field`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and severity tests are added here.

    /// `DiagnosticCode::FieldSampledInvalidConfig` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::FieldSampledInvalidConfig)`.
    /// Pins existence of the new variant for v0.2 sampled-field parse-failure
    /// and runtime-invariant-violation diagnostics.
    #[test]
    fn field_sampled_invalid_config_diagnostic_code_is_constructible() {
        use super::Severity;
        let d = Diagnostic::warning("invalid").with_code(DiagnosticCode::FieldSampledInvalidConfig);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::FieldSampledInvalidConfig));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::FieldSampledInvalidConfig`
    /// serializes as `"FieldSampledInvalidConfig"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_field_sampled_invalid_config_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::FieldSampledInvalidConfig).unwrap();
        assert_eq!(s, "\"FieldSampledInvalidConfig\"");
    }

    // --- TopologyAttributeAmbiguousAfterSplit tests (task 2721 — W_TOPOLOGY_ATTRIBUTE_AMBIGUOUS_AFTER_SPLIT) ---
    // Pairs with `emit_split_children_diagnostic` in
    // `crates/reify-eval/src/topology_attribute_resolver.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip, severity, and serde wire-format tests are added here.

    /// `DiagnosticCode::TopologyAttributeAmbiguousAfterSplit` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::TopologyAttributeAmbiguousAfterSplit)`.
    /// Pins the warning-severity contract and variant existence for the typed
    /// disambiguation of the post-split-cluster outcome (`AttributeResolution::AmbiguousAfterSplit`).
    #[test]
    fn diagnostic_code_topology_attribute_ambiguous_after_split_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::warning("x")
            .with_code(DiagnosticCode::TopologyAttributeAmbiguousAfterSplit);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TopologyAttributeAmbiguousAfterSplit)
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TopologyAttributeAmbiguousAfterSplit`
    /// serializes as `"TopologyAttributeAmbiguousAfterSplit"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_topology_attribute_ambiguous_after_split_serde_pascal_case() {
        let s =
            serde_json::to_string(&DiagnosticCode::TopologyAttributeAmbiguousAfterSplit).unwrap();
        assert_eq!(s, "\"TopologyAttributeAmbiguousAfterSplit\"");
    }

    // --- TopologyAttributeLocalIndexReassigned tests (task 2654 — W_TOPOLOGY_ATTRIBUTE_LOCAL_INDEX_REASSIGNED) ---
    // Pairs with the local-index reassignment detector to be wired in
    // `crates/reify-eval/src/topology_attribute_propagation.rs` in a follow-up
    // step of task #2654. Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug
    // derives are already covered by `diagnostic_code_derives` above; only the
    // variant-specific round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::TopologyAttributeLocalIndexReassigned` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned)`.
    /// Pins the warning-severity contract and variant existence for the typed
    /// disambiguation of the ordering-shuffle rebind outcome (no split, same
    /// `(feature_id, role, user_label)`, different resolved `local_index`).
    #[test]
    fn diagnostic_code_topology_attribute_local_index_reassigned_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::warning("x")
            .with_code(DiagnosticCode::TopologyAttributeLocalIndexReassigned);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TopologyAttributeLocalIndexReassigned)
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TopologyAttributeLocalIndexReassigned`
    /// serializes as `"TopologyAttributeLocalIndexReassigned"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_topology_attribute_local_index_reassigned_serde_pascal_case() {
        let s =
            serde_json::to_string(&DiagnosticCode::TopologyAttributeLocalIndexReassigned).unwrap();
        assert_eq!(s, "\"TopologyAttributeLocalIndexReassigned\"");
    }

    // --- ImportedTolerancePromiseInsufficient tests (task 2651 — W_IMPORTED_TOLERANCE_INSUFFICIENT) ---
    // Pairs with the imported-geometry tolerance-promise checker in
    // `crates/reify-eval/src/tolerance_promise.rs::imported_tolerance_promise_diagnostic`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::ImportedTolerancePromiseInsufficient` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::ImportedTolerancePromiseInsufficient)`.
    /// Pins the warning-severity contract and variant existence for the imported-geometry
    /// tolerance-promise insufficient signal (PRD `docs/prds/v0_2/per-purpose-tolerance.md`
    /// §"Imported geometry promise"; arch §10.4 / §14.5).
    #[test]
    fn diagnostic_code_imported_tolerance_promise_insufficient_with_code_round_trips() {
        let d = Diagnostic::warning("x")
            .with_code(DiagnosticCode::ImportedTolerancePromiseInsufficient);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::ImportedTolerancePromiseInsufficient)
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::ImportedTolerancePromiseInsufficient`
    /// serializes as `"ImportedTolerancePromiseInsufficient"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_imported_tolerance_promise_insufficient_serde_pascal_case() {
        let s =
            serde_json::to_string(&DiagnosticCode::ImportedTolerancePromiseInsufficient).unwrap();
        assert_eq!(s, "\"ImportedTolerancePromiseInsufficient\"");
    }

    // --- InputTolerancePromiseIsZero tests (task 2833 — W_INPUT_TOLERANCE_PROMISE_IS_ZERO) ---
    // Pairs with the imported-geometry zero-promise lint in
    // `crates/reify-eval/src/tolerance_promise.rs::input_tolerance_promise_is_zero_diagnostic`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::InputTolerancePromiseIsZero` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying
    /// `Some(DiagnosticCode::InputTolerancePromiseIsZero)`.
    /// Pins the warning-severity contract and variant existence for the
    /// imported-geometry zero-promise lint (task 2833 — option-(b continuation);
    /// PRD `docs/prds/v0_2/per-purpose-tolerance.md`
    /// §"Resolved design decisions" → "Imported geometry promise").
    #[test]
    fn diagnostic_code_input_tolerance_promise_is_zero_with_code_round_trips() {
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::InputTolerancePromiseIsZero);
        assert_eq!(d.code, Some(DiagnosticCode::InputTolerancePromiseIsZero));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::InputTolerancePromiseIsZero`
    /// serializes as `"InputTolerancePromiseIsZero"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_input_tolerance_promise_is_zero_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::InputTolerancePromiseIsZero).unwrap();
        assert_eq!(s, "\"InputTolerancePromiseIsZero\"");
    }

    // --- LongChainRealization tests (task 2646 — W_LONG_CHAIN_REALIZATION) ---
    // Pairs with the dispatcher long-chain diagnostic in
    // `crates/reify-eval/src/dispatcher.rs::long_chain_diagnostic`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::LongChainRealization` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::LongChainRealization)`.
    /// Pins the warning-severity contract and variant existence for the
    /// dispatcher's long-chain realization diagnostic (PRD
    /// `docs/prds/v0_2/multi-kernel.md` §"Long-chain diagnostic" +
    /// `docs/prds/v0_2/per-purpose-tolerance.md` §"Long-chain diagnostic gating").
    #[test]
    fn diagnostic_code_long_chain_realization_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::LongChainRealization);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::LongChainRealization));
        assert_eq!(
            format!("{:?}", DiagnosticCode::LongChainRealization),
            "LongChainRealization"
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::LongChainRealization`
    /// serializes as `"LongChainRealization"` (PascalCase, from the existing
    /// `rename_all = "PascalCase"` derive on the enum). Pins the wire-format
    /// contract for downstream consumers (LSP / MCP) so a future variant
    /// rename is caught at the wire boundary, not buried in a downstream
    /// integration test.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_long_chain_realization_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::LongChainRealization).unwrap();
        assert_eq!(s, "\"LongChainRealization\"");
    }

    // --- AutoTypeParamDepthBoundExceeded tests (task 2659 — v0.2 backtracking) ---
    // Pairs with the depth-bound producer in
    // `crates/reify-compiler/src/auto_type_param.rs::resolve_auto_type_params_with_backtracking`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // serde wire-form round-trip is added here to lock the LSP/MCP contract.

    /// `DiagnosticCode::AutoTypeParamDepthBoundExceeded` round-trips through
    /// serde under `feature = "serde"`: the wire form is the PascalCase
    /// string `"AutoTypeParamDepthBoundExceeded"`, and deserializing that
    /// string back yields the original variant. Pins both directions of the
    /// LSP/MCP wire contract — the v0.2 BFS-fallback warning is consumed by
    /// downstream tooling that match-arms on this exact wire identifier.
    #[cfg(feature = "serde")]
    #[test]
    fn auto_type_param_depth_bound_exceeded_round_trips_via_serde() {
        let s = serde_json::to_string(&DiagnosticCode::AutoTypeParamDepthBoundExceeded).unwrap();
        assert_eq!(
            s, "\"AutoTypeParamDepthBoundExceeded\"",
            "serde wire form must equal PascalCase identifier"
        );
        let back: DiagnosticCode = serde_json::from_str(&s).unwrap();
        assert_eq!(
            back,
            DiagnosticCode::AutoTypeParamDepthBoundExceeded,
            "deserialize must round-trip back to AutoTypeParamDepthBoundExceeded"
        );
    }

    // --- Multi-kernel dispatch failure variant tests (task 3434) ---
    //
    // Pairs with the five builders in
    // `crates/reify-eval/src/dispatcher.rs` (no_kernel_chain_diagnostic,
    // kernel_pragma_unsatisfiable_diagnostic, pinned_kernel_missing_diagnostic,
    // unpinned_kernel_loaded_diagnostic, kernel_version_mismatch_diagnostic)
    // per PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task γ.
    //
    // Test surface is split deliberately:
    //   • Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives —
    //     already covered by `diagnostic_code_derives` above.
    //   • Per-variant severity + code round-trip — already pinned by each
    //     `<builder>_carries_<severity>_severity_and_code` test in
    //     `crates/reify-eval/src/dispatcher.rs` (the builders construct
    //     `Diagnostic::error(...).with_code(...)` /
    //     `Diagnostic::warning(...).with_code(...)`, so the dispatcher-side
    //     assertion `(severity, code) == (expected, Some(variant))` is the
    //     load-bearing severity pin).
    //   • Per-variant serde wire form — consolidated into the single
    //     table-driven test below so adding a sixth variant under this PRD
    //     chain is a one-line extension to the table (and so a future
    //     reviewer-flagged rename catches all variants at once).

    /// Under `feature = "serde"`, every multi-kernel-phase-3 `DiagnosticCode`
    /// variant serializes to its PascalCase identifier (inherited from
    /// `rename_all = "PascalCase"` on the enum) and round-trips back via
    /// deserialization. Pins the LSP / MCP wire contract for all five new
    /// variants in one place — a future variant rename (or accidental
    /// `rename_all` removal) loudly fails this single test rather than
    /// silently passing five near-identical templates.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_multi_kernel_variants_serde_pascal_case() {
        let cases: &[(DiagnosticCode, &str)] = &[
            (DiagnosticCode::NoKernelChain, "\"NoKernelChain\""),
            (
                DiagnosticCode::KernelPragmaUnsatisfiable,
                "\"KernelPragmaUnsatisfiable\"",
            ),
            (
                DiagnosticCode::PinnedKernelMissing,
                "\"PinnedKernelMissing\"",
            ),
            (
                DiagnosticCode::UnpinnedKernelLoaded,
                "\"UnpinnedKernelLoaded\"",
            ),
            (
                DiagnosticCode::KernelVersionMismatch,
                "\"KernelVersionMismatch\"",
            ),
        ];
        for (variant, expected) in cases {
            let got = serde_json::to_string(variant).unwrap();
            assert_eq!(
                &got, expected,
                "serde wire form for {variant:?} must equal PascalCase identifier",
            );
            let back: DiagnosticCode = serde_json::from_str(&got).unwrap();
            assert_eq!(
                &back, variant,
                "deserialize must round-trip back to the original {variant:?}",
            );
        }
    }

    // --- UnresolvedType tests (task 3721 — E_UNRESOLVED_TYPE) ---
    // Pairs with every "unresolved type" emit site across the compiler crate:
    // functions.rs (param, return, field domain/codomain), guards.rs, entity.rs,
    // expr.rs (lambda param), traits.rs, conformance/checker.rs, type_resolution.rs.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::UnresolvedType` round-trips through
    /// `Diagnostic::error(...).with_code(...)`.
    /// Shape mirrors `diagnostic_code_geometry_unbounded_with_code_round_trips`
    /// (which targets a different variant); a future enum reorganisation that
    /// drops `UnresolvedType` is caught here.
    #[test]
    fn diagnostic_code_unresolved_type_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::UnresolvedType);
        assert_eq!(d.code, Some(DiagnosticCode::UnresolvedType));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::UnresolvedType` serializes as
    /// `"UnresolvedType"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_unresolved_type_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::UnresolvedType).unwrap();
        assert_eq!(s, "\"UnresolvedType\"");
    }

    // --- UnresolvedName tests (task 3721 — E_UNRESOLVED_NAME) ---
    // Pairs with "unresolved name" emit sites: expr.rs:679 (KEY — unbound identifier
    // in expression context) and annotations.rs:321 (solver-hint collection reference).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::UnresolvedName` round-trips through
    /// `Diagnostic::error(...).with_code(...)`.
    /// Shape mirrors `diagnostic_code_geometry_unbounded_with_code_round_trips`
    /// (which targets a different variant); a future enum reorganisation that
    /// drops `UnresolvedName` is caught here.
    #[test]
    fn diagnostic_code_unresolved_name_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::UnresolvedName);
        assert_eq!(d.code, Some(DiagnosticCode::UnresolvedName));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::UnresolvedName` serializes as
    /// `"UnresolvedName"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_unresolved_name_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::UnresolvedName).unwrap();
        assert_eq!(s, "\"UnresolvedName\"");
    }

    /// Pins per-variant severity + variant-existence at the reify-types layer
    /// for all five multi-kernel-phase-3 variants in one table. Although the
    /// dispatcher-side `<builder>_carries_<severity>_severity_and_code` tests
    /// already exercise these end-to-end through the builders, this
    /// reify-types-local assertion guards against severity / variant drift
    /// when the dispatcher crate is not in the test set (e.g. a cargo check
    /// run scoped to `-p reify-types`). Severity assignments match PRD
    /// `docs/prds/v0_3/multi-kernel-phase-3.md` §5: errors fail the build,
    /// warnings let realization proceed.
    #[test]
    fn diagnostic_code_multi_kernel_variants_with_code_round_trip() {
        use super::Severity;
        let error_variants = [
            DiagnosticCode::NoKernelChain,
            DiagnosticCode::PinnedKernelMissing,
            DiagnosticCode::KernelVersionMismatch,
        ];
        for code in error_variants {
            let d = Diagnostic::error("x").with_code(code);
            assert_eq!(
                d.severity,
                Severity::Error,
                "severity mismatch for {code:?}"
            );
            assert_eq!(d.code, Some(code), "code mismatch for {code:?}");
        }
        let warning_variants = [
            DiagnosticCode::KernelPragmaUnsatisfiable,
            DiagnosticCode::UnpinnedKernelLoaded,
        ];
        for code in warning_variants {
            let d = Diagnostic::warning("x").with_code(code);
            assert_eq!(
                d.severity,
                Severity::Warning,
                "severity mismatch for {code:?}",
            );
            assert_eq!(d.code, Some(code), "code mismatch for {code:?}");
        }
    }

    // --- Stackup DiagnosticCode tests (task 4007) ---

    /// All four §4.4 stackup codes round-trip through
    /// `Diagnostic::error(...).with_code(...)` with `Severity::Error`.
    /// Mirrors the `diagnostic_code_multi_kernel_variants_with_code_round_trip` style.
    #[test]
    fn diagnostic_code_stackup_variants_constructible() {
        use super::Severity;
        let codes = [
            DiagnosticCode::StackupEmptyChain,
            DiagnosticCode::StackupDimMismatch,
            DiagnosticCode::StackupBadSign,
            DiagnosticCode::StackupBadSamples,
        ];
        for code in codes {
            let d = Diagnostic::error("x").with_code(code);
            assert_eq!(d.severity, Severity::Error, "severity mismatch for {code:?}");
            assert_eq!(d.code, Some(code), "code mismatch for {code:?}");
        }
    }

    /// Under `feature = "serde"`, each §4.4 stackup code serializes to its
    /// PascalCase wire string (from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_stackup_variants_serde_pascal_case() {
        let cases = [
            (DiagnosticCode::StackupEmptyChain,  "\"StackupEmptyChain\""),
            (DiagnosticCode::StackupDimMismatch, "\"StackupDimMismatch\""),
            (DiagnosticCode::StackupBadSign,     "\"StackupBadSign\""),
            (DiagnosticCode::StackupBadSamples,  "\"StackupBadSamples\""),
        ];
        for (code, expected) in cases {
            let s = serde_json::to_string(&code).unwrap();
            assert_eq!(s, expected, "serde mismatch for {code:?}");
        }
    }
}

/// A diagnostic (error/warning) projected to human-readable line/column positions.
///
/// This is a presentation type — it holds 1-based `line`/`column` positions
/// derived from `SourceSpan` byte-offsets via `byte_offset_to_line_col`.
/// It lives in reify-types (not reify-mcp) so that the engine layer can produce
/// it without importing from the MCP adapter layer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiagnosticInfo {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
}
