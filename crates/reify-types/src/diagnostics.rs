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
    /// Both `reify_types::byte_offset_to_line_col` and
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
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_field`.
    /// Replaces canonical message:
    /// `"field '<name>' codomain mismatch: declared codomain '<C>', lambda body produces '<T>'"`.
    ///
    /// Emitted when the inferred type of an `analytical` lambda body does not
    /// implicitly convert to the declared codomain type. The human-readable
    /// mnemonic used in PRD prose is `E_FIELD_CODOMAIN_MISMATCH`.
    FieldCodomainMismatch,
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
    /// The [`FeatureTagTable`] that `resolve_unique_by_tag` reads from is
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
    /// builder). Reserved for the closed-chain detector that rejects mechanisms whose
    /// joint-parent graph has a conflict (joint J recorded with two different parents)
    /// or a cycle (DFS reaches J again before reaching the world sentinel).
    ///
    /// Canonical message form:
    /// `"closed kinematic chain detected: joint '<J>' has conflicting parents"` (conflict case)
    /// or `"closed kinematic chain detected: cyclic joint-parent path"` (cycle case).
    ///
    /// The PRD-prose mnemonic for this code is `E_KINEMATIC_CLOSED_CHAIN`
    /// (see `docs/prds/kinematic-constraints.md` task 3 and
    /// `docs/reify-stdlib-reference.md` §13.2).
    ///
    /// TODO: wired by the snapshot/eval-pipeline integration in the task family covering
    /// 2585+. The v0.1 mechanism builder records the error condition (and both joint
    /// paths) on the returned Mechanism `Value::Map` (`error`, `error_path1`,
    /// `error_path2`, `error_message` fields); a follow-on integration translates the
    /// errored Map into a real `Diagnostic` carrying this code via
    /// `EvalResult.diagnostics`. The variant is reserved now so that downstream tooling
    /// (LSP / MCP / IDE error UIs) can match on the typed code identifier from the
    /// moment the diagnostic is emitted, with no further enum churn at integration time.
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
/// use reify_types::{Diagnostic, Severity};
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
        let d = Diagnostic::error("x")
            .with_candidates(vec!["A".to_string(), "B".to_string()]);
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
        assert_eq!(format!("{:?}", DiagnosticCode::GeometryUnbounded), "GeometryUnbounded");
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
