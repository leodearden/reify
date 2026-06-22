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
    /// Origin: `crates/reify-compiler/src/conformance/mod.rs::emit_geometry_profile_required`,
    /// called by the profile-consumer arms in `crates/reify-compiler/src/geometry.rs`
    /// (`extrude`/`extrude_symmetric`/`revolve`/`loft`/`loft_guided`/`sweep`/
    /// `sweep_guided`/`pipe`).
    /// Canonical message form:
    /// `"geometry argument '<name>' must be <requirement>"`.
    ///
    /// Emitted when a profile-consuming geometry op receives a statically-known
    /// operand whose inferred dimensionality refinement violates the op's
    /// precondition: a profile slot requires a 2-D `Surface` (Closed ∧ Planar),
    /// a sweep/pipe path slot requires a 1-D `Curve` (see
    /// [`GeomDim`](../../reify_compiler/geometry_traits_inference/enum.GeomDim.html)).
    /// Permissive (PRD decision 5): the check fires only for operands that are
    /// nested geometry constructors (FunctionCall `CompiledExpr`s resolved via
    /// `try_infer_traits_for_function_call`); `param`/`let` value-refs are
    /// accepted. Non-fatal — the op is still lowered (mirrors [`GeometryUnbounded`]).
    /// See `docs/prds/geometry-primitive-constructors.md` task α.
    GeometryProfileRequired,
    /// Origin: the eval `ModifyKind::Fillet` 3-arg arm in
    /// `crates/reify-eval/src/geometry_ops.rs` (curated-fillet anti-zero-edges guard).
    /// Canonical message form:
    /// `"fillet(...): edge selector resolved to zero edges"`.
    ///
    /// Emitted as a `Severity::Error` when a *present* (3-arg
    /// `fillet(solid, edges, radius)`) edge selector resolves to an **empty**
    /// vector. This is a hard user error: the op must NEVER silently fall through
    /// to the all-edges path (which would fake-complete the build with an
    /// unintended geometry — the task-3295 trap). The 2-arg back-compat form
    /// `fillet(solid, radius)` has no selector argument, so its empty edge list
    /// legitimately means all-edges and does NOT emit this code.
    ///
    /// The PRD-prose mnemonic is `E_EMPTY_SELECTION` (see
    /// `docs/prds/geometry-modify-sweep-completion.md`); per the `E_*` → Error
    /// severity convention this is always a blocking error.
    EmptyEdgeSelection,
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
    /// Replaces two canonical message forms (Undef branch, Severity::Warning):
    ///
    /// - `"constraint <id> indeterminate: undefined inputs: <cells>"` — emitted when
    ///   ≥1 leaf `ValueRef` in the constraint expression resolves to `Value::Undef`
    ///   (data is absent). `<cells>` is a comma-separated list of the undefined cell
    ///   names (deduped, sorted alphabetically) via `ValueCellId::Display`
    ///   (`"entity.member"` format). Note: `collect_value_refs()` also returns
    ///   `CrossSubGeometryRef` IDs; those are treated the same as ordinary cell IDs
    ///   and will appear here if absent from the `ValueMap`.
    ///
    /// - `"constraint <id> indeterminate: operator undefined for these operand kinds: <kinds>"`
    ///   — emitted when all leaf `ValueRef`s are defined but the operator is undefined
    ///   for the given operand kinds (e.g. comparing a Tensor to a Scalar, or comparing
    ///   scalars of mismatched dimensions). `<kinds>` is a comma-separated list of the
    ///   distinct operand kinds (deduped, sorted alphabetically) such as `"Tensor"`,
    ///   `"Scalar<m>"`, `"Enum<MyType>"`. When the expression has no `ValueRef` leaves
    ///   (literal-only), the `": <kinds>"` suffix is omitted.
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
    /// Origin: `crates/reify-compiler/src/entity.rs::expand_constraint_inst`
    ///          (param-level argument type check, task 4546).
    ///
    /// Emitted when a constraint instantiation passes an argument whose
    /// compile-time type is incompatible with the declared parameter type —
    /// specifically, a cross-category mismatch (e.g. `Bool` passed where a
    /// `Length` param is declared). Numeric-to-numeric mismatches (e.g. `Int`
    /// for `Length`) are deliberately tolerated at the binding site; dimensional
    /// strictness within comparison predicates is enforced by task 4490's
    /// `E_CmpOperandKind` guard.
    ///
    /// Canonical message form:
    /// `"type mismatch: argument '<arg>' has type <actual> but parameter '<param>' \
    ///   of constraint '<def>' expects <expected>"`.
    ///
    /// The PRD-prose mnemonic is `E_CONSTRAINT_ARG_TYPE`.
    ConstraintArgTypeMismatch,
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_field`.
    /// Emitted when a field declaration uses the `sampled { ... }` source form,
    /// which is deferred to v0.2 (v0.1 supports `analytical` and `composed` only).
    FieldSampledV02,
    /// Origin: `crates/reify-compiler/src/functions.rs::compile_field`.
    /// Emitted when a field declaration uses the `imported { ... }` source form,
    /// which is deferred to v0.2 (v0.1 supports `analytical` and `composed` only).
    FieldImportedV02,
    /// Origin: `crates/reify-eval/src/engine_eval.rs::elaborate_field` (Imported arm).
    /// Emitted as a `Severity::Error` at eval time when an `imported` field's
    /// source file cannot be read (file not found, wrong grid name, FFI not
    /// compiled in, etc.).  The field's lambda becomes `Value::Undef` and any
    /// subsequent `sample(...)` call returns `Undef`.
    ///
    /// Canonical message form:
    /// `"field '<name>': failed to import VDB file: <detail>"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_FIELD_IMPORT_FAILED`.
    /// Registered in task 3576 (PRD §9 / GR-003 task θ step-8).
    FieldImportFailed,
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
    /// Origin: `crates/reify-expr/src/lib.rs::eval_from_samples`.
    /// Canonical message form:
    /// `"from_samples: points must form a uniformly-spaced 1-D regular grid (<reason>)"`.
    ///
    /// Emitted when the `points` argument to `from_samples(points, values, method)` is not
    /// a valid 1-D regular grid. Reasons include: non-scalar elements (e.g. Point2/Point3),
    /// length mismatch between points and values, fewer than 2 points, non-positive or
    /// non-uniform spacing. The builtin returns `Value::Undef` when this code fires.
    ///
    /// The PRD-prose mnemonic is `E_FIELD_SAMPLES_NOT_GRID`. Severity is `Error`
    /// (unlike the sibling `W_FIELD_SAMPLED_INVALID_CONFIG` which is a Warning).
    FieldSamplesNotGrid,
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
    /// Origin: `crates/reify-compiler/src/entity.rs::check_param_default_type`.
    ///
    /// Canonical message form:
    /// `"parameter '<name>' declared `<declared>` but its initializer evaluates to `<init>`; declared type and initializer dimension must agree"`.
    ///
    /// Emitted when a structure `param` has an explicit type annotation whose
    /// dimension is incompatible with the compiled initializer's `result_type`.
    /// The check uses bidirectional `type_compatible` (Int→Real widening; `Type::Error`
    /// anti-cascade wildcard) and is restricted to scalar-comparable declared types
    /// (`Real | Int | Scalar{..}`). The diagnostic is anchored at `param.span` so
    /// the user sees the offending declaration, not a downstream consumer.
    ///
    /// The human-readable mnemonic used in task prose is `E_PARAM_DEFAULT_TYPE_MISMATCH`.
    ParamDefaultTypeMismatch,
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
    /// Origin: `crates/reify-expr/src/lib.rs::eval_from_samples`.
    /// Canonical message form:
    /// `"from_samples: interpolation method '<variant>' is not supported by from_samples \
    ///  (supported: Linear, NearestNeighbor, Cubic)"`.
    ///
    /// Emitted when the `method` argument to `from_samples(points, values, method)` is a
    /// `Value::Enum { type_name: "InterpolationMethod", .. }` variant that `from_samples`
    /// does not support. Currently fires for `"RBF"` and `"Kriging"` (and any
    /// unrecognized variant). These are HARD errors in `from_samples` — unlike
    /// `interp::resolve_method`, which falls back to Linear with a Warning. The builtin
    /// returns `Value::Undef` when this code fires.
    ///
    /// The PRD-prose mnemonic is `E_INTERP_METHOD_UNSUPPORTED`. Severity is `Error`.
    InterpMethodUnsupported,
    /// Origin: `crates/reify-compiler/src/expr.rs` (binary-op `Add`/`Sub` site and
    ///          range-bounds site), via `crates/reify-compiler/src/type_compat::format_dimension_mismatch_diagnostic`.
    /// Canonical message form: `"dimension mismatch in {op}: {left} vs {right}"`.
    ///
    /// Emitted when two `Type::Scalar` operands carry different, incompatible
    /// dimensions (e.g. Money vs Force). The diagnostic may carry an optional
    /// secondary label naming the canonical dimensions when both are known
    /// (e.g. `"Money and Force are different dimensions and cannot be combined directly"`).
    DimensionMismatch,
    /// Origin: `crates/reify-compiler/src/type_resolution.rs`
    ///          (`resolve_type_expr_with_aliases_kinded`, bare-Scalar guard, task 4375 γ).
    /// Canonical message form:
    /// `"bare \`Scalar\` is not a valid type: write \`Scalar<Q>\` or a named dimension like \`Length\`"`.
    ///
    /// Emitted as `Severity::Error` when the resolver encounters the unparameterized
    /// identifier `Scalar` (i.e. `type_args.is_empty()`) at a type-expression position.
    /// The guard returns `Some(Type::Error)` (poison sentinel) so callers suppress their
    /// generic `UnresolvedType` cascade — the user sees exactly one clean E_BARE_SCALAR
    /// diagnostic rather than two cascaded errors.
    ///
    /// Note: `Scalar<Q>` with valid or invalid type args is **not** covered by this code —
    /// `Scalar<Length>` is fine; `Scalar<NotADimension>` surfaces a precise dimension error
    /// emitted by the parameterized-builtin path. The `type_args.is_empty()` guard enforces
    /// the distinction.
    ///
    /// PRD mnemonic: `E_BARE_SCALAR`. See `docs/prds/v0_6/real-dimensionless-unification.md`.
    BareScalarType,
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
    /// Origin: `crates/reify-compiler/src/units.rs` (`selector_composition_result_type`),
    /// called from `crates/reify-compiler/src/expr.rs` (selector-composition ladder arm).
    /// Emitted as an `Error` when a selector composition (`union`/`intersect`/`difference`)
    /// violates the K1 kind-closure invariant (all operands must share the same
    /// `SelectorKind`).
    ///
    /// Two message variants are emitted under this code (distinguished by message text):
    ///
    /// 1. **Mixed-kind composition** — all operands are selectors but of different kinds
    ///    (e.g. `Face` and `Edge`):
    ///    `"selector composition kind mismatch: cannot compose <KindA> and <KindB>"`
    ///    Label at the composition call site: `"mixed-kind selector composition"`.
    ///
    /// 2. **Non-selector operand mixed with selectors** — at least one operand is not a
    ///    `Type::Selector` at all (e.g. `union(faces(b), box(…))`):
    ///    `"selector composition requires all operands to be selectors; N non-selector \
    ///     operand(s) found"`
    ///    Label at the composition call site: `"non-selector operand in selector composition"`.
    ///
    /// Exactly one diagnostic is emitted per composition call site in both cases; the
    /// result type is inferred as `Type::Selector(first_kind)` for downstream anti-cascade.
    ///
    /// The PRD-prose mnemonic for this code is `E_SELECTOR_KIND_MISMATCH`
    /// (see `docs/prds/topology-selector-value-type.md` §11.2).
    SelectorKindMismatch,
    /// Origin: `crates/reify-compiler/src/builtin_signatures.rs` (task 4493,
    /// type-hygiene ζ).
    /// Emitted as `Severity::Error` when a call site passes a statically-known
    /// argument whose type is a DEFINITE mismatch with the builtin's expected
    /// dimensioned-scalar arg type.
    ///
    /// Canonical message form:
    /// `"{builtin}: {arg_name} argument expects {type_name}, got {actual}"`
    ///
    /// where:
    /// - `{builtin}` — the builtin function name (e.g., `"moment_of_inertia"`).
    /// - `{arg_name}` — the parameter name (e.g., `"density"`, `"tol"`, `"h"`).
    /// - `{type_name}` — the expected physical quantity name (e.g., `"Density"`,
    ///   `"Angle"`, `"Length"`), mirroring the γ runtime `ArgRejection::message`
    ///   wording so compile-time and runtime diagnostics read consistently.
    /// - `{actual}` — the actual resolved argument type (`Type::Display`), e.g.,
    ///   `"Real"` for a bare `7850.0`, `"Bool"` for a boolean, `"Scalar[m]"` for
    ///   a length scalar.
    ///
    /// Distinct from [`DimensionMismatch`] (which has Add/Sub-specific semantics)
    /// and [`SelectorKindMismatch`] (selector-composition invariant): minted as a
    /// dedicated code because the builtin-arg mismatch can be a *kind* mismatch
    /// (e.g., `Bool` where `Density` is expected), not only a dimension mismatch,
    /// and because it names the builtin's parameter contract rather than an
    /// operator-level invariant — following the `SelectorKindMismatch` minting
    /// precedent (diagnostics.rs §"minting rationale").
    ///
    /// Gradualism (PRD decision 6): `Type::Error` (poison) and `Type::TypeParam`
    /// (unresolved generic) are silently skipped; only concrete types fire.
    ///
    /// The PRD-prose mnemonic for this code is `E_ARG_TYPE_MISMATCH`
    /// (see `docs/prds/type-hygiene.md` ζ §"Compile-time arg-type guard").
    ArgTypeMismatch,
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
    /// Origin: `crates/reify-eval/src/engine_build.rs::execute_realization_ops`
    /// (via `diagnose_topology_correspondence_drops`).
    ///
    /// Emitted as `Severity::Warning` when a kernel history record reports a
    /// non-zero topology-correspondence-loss counter after a boolean, sweep, or
    /// local-feature operation. The following counters are covered:
    ///
    /// - `BooleanOpHistoryRecords::silent_drop_count` — a child subshape was
    ///   absent from the kernel's result correspondence map.
    /// - `SweepOpHistoryRecords::silent_drop_count` — same for sweep ops
    ///   (extrude / revolve / sweep).
    /// - `SweepOpHistoryRecords::unsynthesized_profile_edge_count` — a profile
    ///   edge produced no result-face correspondence record.
    /// - `SweepOpHistoryRecords::duplicate_parent_subshape_index_count` — a
    ///   generated-face correspondence record was dropped by dedup.
    /// - `LocalFeatureOpHistoryRecords::silent_drop_count` — same for fillet /
    ///   chamfer ops.
    ///
    /// All five counter kinds share this single code; the specific counter and
    /// count are named in the diagnostic message. The geometry is valid; only
    /// persistent-naming correspondence tracking is degraded.
    ///
    /// Canonical message form:
    /// `"topology correspondence dropped: {op_kind} {counter_name}={count} context={context}"`
    ///
    /// The PRD-prose mnemonic for this code is `W_TOPOLOGY_CORRESPONDENCE_DROPPED`.
    TopologyCorrespondenceDropped,
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
    /// `"auto type-parameter search exceeded depth bound: <N> auto-type-params declared, max_depth = <M>; falling back to per-parameter BFS (v0.1 algorithm). NOTE: the BFS fallback is sound - a jointly-infeasible assignment is rejected with a hard E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE error (joint-recheck, #4434), so no wrong substitution is silently accepted - but BFS is less COMPLETE than the full DFS over the cross-product, so a feasible binding that DFS would find may be missed; raise the configured bound to recover completeness."`
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
    /// `"auto type-parameter cross-product search exceeded size cap: <N> auto-type-params declared (<P1>, <P2>, ...) with per-param candidate counts [<k1>, <k2>, ...] yielding cross-product size <S>, max_cross_product_size = <C>; falling back to per-parameter BFS (v0.1 algorithm). NOTE: the BFS fallback is sound - a jointly-infeasible assignment is rejected with a hard E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE error (joint-recheck, #4434), so no wrong substitution is silently accepted - but BFS is less COMPLETE than the full DFS over the cross-product, so a feasible binding that DFS would find may be missed; raise the configured bound to recover completeness."`
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
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs::emit_fallback_warning_and_delegate_to_bfs`.
    ///
    /// Canonical message form:
    /// `"auto type-parameter BFS fallback assignment is jointly infeasible:
    /// parameters [<names>] exceed <bound> (depth bound max_depth=<N>|cross-product
    /// cap max_cross_product_size=<C>); BFS assignment [<T=fqn>, …] violates
    /// constraint(s) [<id>, …] under joint check. No substitution produced."`
    ///
    /// Where:
    /// - `<names>` lists the `auto:` type-parameter names (declared order).
    /// - The `<bound>` clause identifies which fallback fired: either
    ///   `depth bound max_depth=<N>` (from `AutoTypeParamDepthBoundExceeded`)
    ///   or `cross-product cap max_cross_product_size=<C>` (from
    ///   `AutoTypeParamCrossProductSizeExceeded`) — derived from the `code`
    ///   argument passed to the helper.
    /// - `[<T=fqn>, …]` is the per-param assignment BFS returned.
    /// - `[<id>, …]` are the `ConstraintNodeId`s that returned `Violated`
    ///   in the single joint `check_constraints_leaf` call.
    ///
    /// Emitted as `Severity::Error` when:
    /// 1. The v0.2 DFS-over-cross-product falls back to v0.1 BFS (depth-bound
    ///    or cross-product-cap guard fires).
    /// 2. BFS returns a COMPLETE assignment (`substitution.len() == params.len()`).
    /// 3. The joint recheck (`check_constraints_leaf` with a full ValueMap seeded
    ///    from all candidates' literal defaults via `seed_candidate_value_map`)
    ///    finds at least one `Violated` constraint.
    ///
    /// On this path **no substitution is produced** for the declaration — the
    /// BFS assignment is discarded entirely because it is jointly infeasible.
    /// The caller returns a `MultiParamResolutionOutcome` with an empty
    /// `substitution`.  The Error is emitted INSTEAD of (not in addition to)
    /// the depth-bound/cap `Warning`.
    ///
    /// Severity is `Error` (not `Warning`) because the BFS assignment is
    /// unsound: accepting it would substitute a cross-product-infeasible
    /// combination into the parameterized template.  A Warning would
    /// mis-signal that compilation succeeded.
    ///
    /// The PRD-prose mnemonic for this code is
    /// `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` (see
    /// `docs/prds/v0_3/auto-type-param-resolution-completion.md` §6.2).
    AutoTypeParamBoundedInfeasible,
    /// Origin: `crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs`
    /// (monomorph-build pass, per-cell synthesis guard — task 4435 δ).
    ///
    /// Canonical message form:
    /// `"auto type parameter resolved candidate '<Candidate>' is not constructible: \
    ///   required parameter '<param>' has no default; cannot synthesize a zero-arg \
    ///   instance for 'param <member> : <T>'"`.
    ///
    /// Emitted as `Severity::Error` when the monomorph-build pass finds that a
    /// resolved candidate has ≥1 required (non-defaulted) `Param` cell.
    /// A zero-arg StructureInstanceCtor synthesized over such a candidate would
    /// produce a `Value::StructureInstance` silently missing the required field —
    /// a fake-completion trap.  The Error names the first missing param so the
    /// user can provide an explicit default or a zero-arg-constructible candidate.
    ///
    /// The cell's `default_expr` is left `None` (no synthesized ctor), preserving
    /// the existing `Value::Undef` fallthrough at `unfold.rs` for that cell.
    ///
    /// The PRD-prose mnemonic for this code is
    /// `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE` (see
    /// `docs/prds/v0_3/auto-type-param-resolution-completion.md` §δ).
    AutoTypeParamCandidateNotConstructible,
    /// Origin: `crates/reify-compiler/src/auto_type_param.rs::emit_unevaluated_constraint_warnings`.
    ///
    /// Canonical message form (as emitted by `emit_unevaluated_constraint_warnings`):
    /// `"auto: constraint '{entity}[{idx}]' reads cell '{cell}' whose default is \
    ///   a computed expression not reducible at compile time; the cell is skipped \
    ///   by the literal-only seeder so the constraint evaluates to Indeterminate \
    ///   (W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED — Gap-C, task #4616)"`.
    ///
    /// Where `{entity}[{idx}]` identifies the constraint (entity name + index
    /// within that entity) and `{cell}` is the member name of the skipped cell.
    ///
    /// Emitted as `Severity::Warning` when the template-side literal-only seeder
    /// (`seed_template_literal_params`) skips a cell whose `default_expr` is a
    /// computed (non-literal) expression (Gap C), and that cell is referenced by
    /// an `auto:` resolution constraint. Because the seeder leaves the cell
    /// unseeded, the constraint's evaluation is `Indeterminate` — treated as
    /// feasible by the resolver's monotonic design (arch §2.5: only `Violated`
    /// rejects a candidate) — so the constraint provides no filtering signal.
    /// The warning names the constraint and the computed-default cell so the
    /// user can inspect the precision loss without a selection-outcome change.
    ///
    /// Severity is `Warning` (not `Error`) because the monotonic feasibility
    /// rule keeps selection sound: `Indeterminate = feasible` never picks an
    /// infeasible candidate — it only loses precision. Invariant 3 (selection
    /// outcome unchanged) is preserved — the warning is informational.
    ///
    /// The PRD-prose mnemonic for this code is
    /// `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED`
    /// (see `docs/prds/v0_3/auto-type-param-constraint-seeding-gaps.md` §6).
    AutoTypeParamConstraintUnevaluated,
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
    /// builder). Emitted when a `body()` call attaches a solid that is already recorded
    /// in the same Mechanism (detected by structural `Value::Eq`; the docs spec says
    /// referential identity — gap documented in mechanism.rs and tracked in task 2538).
    ///
    /// Canonical message form (sourced verbatim from the Map's `error_message` field,
    /// as produced by `make_duplicate_solid_error` in `mechanism.rs`):
    /// `"duplicate solid: solid value already attached to a body in this mechanism"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_MECHANISM_DUPLICATE_SOLID`
    /// (see `docs/prds/kinematic-constraints.md` task 3 and
    /// `docs/reify-stdlib-reference.md` §13.2).
    ///
    /// **Emitted** at the `reify-eval` eval boundary by
    /// `detect_mechanism_errors` in `engine_eval.rs` (task 4308), which scans
    /// the evaluated `ValueMap` for mechanism Maps carrying `error="duplicate_solid"`
    /// and maps each distinct errored Map to one `Severity::Error` diagnostic.
    /// Wired into both `Engine::eval` and `Engine::eval_cached` so the error
    /// surfaces on `reify check` (no kernel) and in the GUI diagnostics panel.
    MechanismDuplicateSolid,
    /// Origin (L1/eval): `crates/reify-stdlib/src/snapshot.rs` — `bind` arm
    /// guard (task 4309 α) and `crates/reify-stdlib/src/sweep.rs` — `dim` /
    /// `sweep` / `sweep_grid` arm guards (task 4309 α).
    /// Origin (L2/compile): `crates/reify-compiler` — DrivingJoint-bound
    /// enforcement (task γ; not yet landed).
    ///
    /// Canonical message form:
    /// `"non-driving joint passed to bind/dim/sweep: coupling and fixed joints \
    ///   have no free motion variable; use a driving joint (prismatic, revolute, \
    ///   cylindrical, planar, or spherical)"`.
    ///
    /// Emitted as a `Severity::Error` by `detect_nondriving_joint_errors` in
    /// `engine_eval.rs` (task 4309) when any top-level evaluated cell holds a
    /// `Value::Map` carrying `error = "nondriving_joint"`.  Wired into both
    /// `Engine::eval` and `Engine::eval_cached` so the diagnostic surfaces on
    /// `reify check` (no kernel) and in the GUI diagnostics panel.
    ///
    /// Per PRD D6: **one code, two emission sites** — L1 (eval, task α/4309)
    /// and L2 (compile, task γ/4310) share this same variant.
    ///   - L1: `detect_nondriving_joint_errors` in `engine_eval.rs` (task α) — landed
    ///   - L2: `check_expr_mechanism_joint_bound` in `conformance/mod.rs` (task γ) — **LANDED**
    ///
    /// The PRD-prose mnemonic for this code is `E_MECHANISM_NONDRIVING_JOINT`.
    MechanismNonDrivingJoint,
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
    /// Note: surfaced through the snapshot/sweep API once snapshot-evaluator
    /// integration lands — `reify-stdlib::snapshot` and the eval engine do not
    /// yet call the wrapper. The variant is reserved now so downstream tooling
    /// (LSP / MCP / IDE error UIs) can match on the typed code identifier from
    /// the moment the diagnostic is first emitted, with no further enum churn
    /// at integration time.
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
    /// Note: surfaced through the snapshot/sweep API once snapshot-evaluator
    /// integration lands. Reserved now for typed-code matching at the moment
    /// the diagnostic is first emitted.
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
    /// Note: surfaced through the snapshot/sweep API once snapshot-evaluator
    /// integration lands. Reserved now for typed-code matching at the moment
    /// the diagnostic is first emitted.
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
    /// A generic function signature references a type name that is neither a
    /// declared type parameter of that function nor a known type alias, builtin,
    /// or structure.
    ///
    /// Origin site: `crates/reify-compiler/src/functions.rs::compile_function`
    /// (param-type and return-type resolution failure arms, gated on
    /// `!type_param_names.is_empty()`).
    ///
    /// Only emitted when the enclosing function IS generic (`<T, …>`). Non-generic
    /// functions with an unknown type name continue to emit `UnresolvedType` so
    /// that the existing "unresolved type: <name>" message and code are preserved
    /// bit-for-bit (INV-6 regression pin — see `fn_generic_signature_tests.rs`
    /// `nongeneric_unknown_type_keeps_unresolved_type`).
    ///
    /// Canonical message form:
    /// `"type '<expr>' in the signature of generic function '<name>' is not a declared type parameter or a known type"`
    ///
    /// The PRD-prose mnemonic for this code is `E_FN_UNKNOWN_TYPE_PARAM`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FnUnknownTypeParam,
    /// A non-dimension-kinded type parameter is used in a dimension slot
    /// (`Scalar<T>`, `Vector3<T>`, or `Point3<T>` where `T` is not declared
    /// with a `Dimension` bound), OR a dimension-kinded type parameter is
    /// used as an ordinary type (bare `Q` in a non-dimension position).
    ///
    /// Both misuse cases produce a single root-cause DimParamKind Error and
    /// return `Some(Type::Error)` so no competing `FnUnknownTypeParam` or
    /// `UnresolvedType` Error is emitted (anti-cascade pattern).
    ///
    /// Origin site: `crates/reify-compiler/src/type_resolution.rs` —
    /// `resolve_parameterized_builtin_type` (Scalar/Vector3/Point3 arms via the
    /// `try_dim_param_slot_or_kind_error` classifier) and
    /// `resolve_type_expr_with_aliases_kinded` (bare-name path).
    ///
    /// The PRD-prose mnemonic for this code is `E_DIM_PARAM_KIND`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    DimParamKind,
    /// A generic function call binds the same type parameter to two different
    /// concrete types across its arguments (call-site type-argument inference
    /// conflict).
    ///
    /// Origin site: `crates/reify-compiler/src/expr.rs::compile_expr_guarded`
    /// (the `OverloadResolution::Resolved` arm for a generic callee), emitted
    /// when the call-site `type_compat::unify` pass returns
    /// `Err(TypeArgConflict)` — i.e. an earlier argument bound type parameter
    /// `P` to one type and a later argument requires a different one.
    ///
    /// Only reachable for generic user functions (`fn f<T>(…)`); non-generic
    /// calls bypass unification entirely (INV-6).
    ///
    /// Canonical message form:
    /// `"conflicting type arguments for type parameter '<P>' in call to '<name>': <existing> vs <incoming>"`
    ///
    /// The PRD-prose mnemonic for this code is `E_FN_TYPE_ARG_CONFLICT`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FnTypeArgConflict,
    /// An Option-recovery or Map-recovery combinator call supplies a default
    /// value whose type does not unify with the subject's element type.
    /// The authoritative combinator set is `FALLBACK_COMBINATORS` in
    /// `crates/reify-compiler/src/expr.rs` (currently: `unwrap_or`,
    /// `or_default`, `fallback`, `get_or`).  Edit that constant to add or
    /// remove members — the doc comments here do not need to be kept in sync.
    ///
    /// Origin site: `crates/reify-compiler/src/expr.rs::compile_expr_guarded`
    /// (the `OverloadResolution::Resolved` generic type-arg-conflict arm),
    /// emitted when `type_compat::unify` returns `Err(TypeArgConflict)` for a
    /// call whose name is in the recovery-combinator set
    /// (`is_fallback_combinator`).  The conflict is between the default-value
    /// argument type and the element type bound by the subject argument.
    ///
    /// Canonical message form:
    /// `"E_FALLBACK_TYPE: conflicting type arguments for type parameter '<P>' in call to '<name>': <existing> vs <incoming>"`
    ///
    /// The PRD-prose mnemonic is `E_FALLBACK_TYPE`; contract C-3 of
    /// `docs/prds/v0_6/result-and-fallback.md`.
    FallbackType,
    /// A generic function call's type argument(s) cannot be inferred from the
    /// supplied arguments, leaving the call's result type wholly undetermined.
    ///
    /// Origin site: `crates/reify-compiler/src/expr.rs::compile_expr_guarded`
    /// (the `OverloadResolution::Resolved` arm for a generic callee), emitted
    /// when the fully-substituted return type is a BARE top-level
    /// `Type::TypeParam(_)` — nothing in the arguments pinned it (e.g.
    /// `fn make<T>() -> T` called as `make()`). A NESTED unbound parameter
    /// (e.g. `Field<TypeParam(D), Real>`) is tolerated, since an enclosing call
    /// can still pin it.
    ///
    /// Only reachable for generic user functions (`fn f<T>(…)`); non-generic
    /// calls keep `return_type.clone()` verbatim (INV-6).
    ///
    /// Canonical message form:
    /// `"cannot infer type argument(s) for generic call to '<name>': result type is undetermined"`
    ///
    /// The PRD-prose mnemonic for this code is `E_FN_TYPE_ARG_UNRESOLVED`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FnTypeArgUnresolved,
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
    /// The remaining six PRD §7 codes were added in task ε (#3837).
    ///
    /// The PRD-prose mnemonic for this code is `E_SHELL_BAD_THRESHOLD`.
    ShellBadThreshold,
    /// Origin: `crates/reify-eval/src/shell_extract_compute.rs` (ε trampoline,
    /// medial-mask / mid-surface phase, `GridValidationError::EmptyAxisGrid`
    /// arm).
    ///
    /// Emitted as `Severity::Error` when the `SampledField` supplied to the
    /// `"shell-extract::extract"` trampoline has an empty axis grid along one
    /// or more dimensions (i.e. no voxel grid exists). The producer cannot
    /// compute a medial mask or extract a mid-surface from a zero-extent grid.
    ///
    /// Illustrative message form (Phase 1 — medial-mask):
    /// `"shell-extract::extract: voxel grid is empty on axis {axis}; cannot compute medial mask. Verify the body geometry produces a valid voxel grid."`.
    ///
    /// Illustrative message form (Phase 2 — mid-surface):
    /// `"shell-extract::extract: voxel grid is empty on axis {axis}; cannot extract mid-surface. Verify the body geometry produces a valid voxel grid."`.
    ///
    /// The exact text is not a stable contract — use the `code` field to match
    /// programmatically. The PRD-prose mnemonic for this code is
    /// `E_SHELL_NO_VOXEL_GRID` (severity convention: `W_*` → Warning, `E_*` → Error).
    ShellNoVoxelGrid,
    /// Origin: `crates/reify-eval/src/shell_extract_compute.rs` (ε trampoline,
    /// mid-surface phase, `MidSurfaceError::MaskVoxelOutOfBounds` arm).
    ///
    /// Emitted as `Severity::Error` when the medial-axis mask contains a voxel
    /// index that falls outside the SDF grid bounds during mid-surface
    /// extraction. This indicates an internal grid/mask size mismatch.
    ///
    /// Illustrative message form:
    /// `"shell-extract::extract: medial-mask voxel [{vx}, {vy}, {vz}] is outside the SDF grid extent [{ex}, {ey}, {ez}]."`.
    ///
    /// The exact text is not a stable contract — use the `code` field to match
    /// programmatically. The PRD-prose mnemonic for this code is
    /// `E_SHELL_MEDIAL_MASK_OOB` (severity convention: `W_*` → Warning, `E_*` → Error).
    ShellMedialMaskOob,
    /// Origin: `crates/reify-eval/src/shell_extract_compute.rs` (ε trampoline,
    /// branch-pruning phase, any `PruneError` arm).
    ///
    /// Emitted as `Severity::Error` when the branch-pruning step on the raw
    /// mid-surface mesh fails. Pruning removes dangling branches from the
    /// medial-surface skeleton; a failure here indicates an ill-conditioned
    /// mesh or degenerate geometry.
    ///
    /// Illustrative message form:
    /// `"shell-extract::extract: branch-pruning failed: {prune_error}"`.
    ///
    /// The exact text is not a stable contract — use the `code` field to match
    /// programmatically. The PRD-prose mnemonic for this code is
    /// `E_SHELL_PRUNE_FAILED` (severity convention: `W_*` → Warning, `E_*` → Error).
    ShellPruneFailed,
    /// Origin: `crates/reify-eval/src/shell_extract_compute.rs` (ε trampoline,
    /// meshing phase, `MesherError::QualityBelowThreshold` arm).
    ///
    /// Emitted as `Severity::Error` when the mid-surface mesher produces a mesh
    /// whose worst-element quality falls below the configured `min_angle_degrees`
    /// threshold, making the mesh unusable for FEA.
    ///
    /// Illustrative message form:
    /// `"shell-extract::extract: mid-surface mesh quality is below threshold (worst aspect ratio: {min_aspect_ratio:.4}, worst min angle: {min_angle_degrees:.2}°). The shell geometry may be too complex or degenerate for meshing."`.
    ///
    /// The exact text is not a stable contract — use the `code` field to match
    /// programmatically. The PRD-prose mnemonic for this code is
    /// `E_SHELL_MESH_QUALITY` (severity convention: `W_*` → Warning, `E_*` → Error).
    ShellMeshQuality,
    /// Origin: `crates/reify-eval/src/compute_targets/elastic_static.rs`
    /// (`solve_elastic_static_trampoline` — too-thick dispatch-site policy,
    /// added in task ε #3837).
    ///
    /// Emitted when a body's thickness/extent ratio (`height / min(length,
    /// width)`) is ≥ `shell_threshold` (default 0.2), indicating the body is
    /// too thick to be meaningfully solved as a thin shell.
    ///
    /// - `ShellForce::On` (`@shell`): emitted as `Severity::Error`; the solve
    ///   aborts immediately with no tet fallback (`FailurePolicy::HardError`).
    ///   PRD-prose mnemonic `E_SHELL_TOO_THICK`.
    /// - `ShellForce::Auto`: emitted as `Severity::Warning`; the solve falls
    ///   back to the tet/solid path (`FailurePolicy::TetFallbackWithWarning`).
    ///
    /// Canonical message form:
    /// `"body thickness/extent ratio <ratio:.2> ≥ shell_threshold <threshold:.2>: body is too thick for shell solve (ratio must be < <threshold:.2>). Use ElasticOptions(shell_force: ShellForce.Off) / @solid to suppress this <error/warning>."`.
    ///
    /// The PRD-prose mnemonic for this code is `E_SHELL_TOO_THICK` /
    /// `W_SHELL_TOO_THICK` depending on severity.
    ShellTooThick,
    /// Origin: `crates/reify-eval/src/shell_extract_compute.rs`
    /// (`shell_extract_compute_fn`, medial-mask phase, empty-mask guard).
    ///
    /// Emitted as `Severity::Error` when `compute_medial_mask` succeeds but
    /// returns a mask with zero medial voxels (geometry fully solid or voxel
    /// resolution too coarse), short-circuiting before mid-surface extraction.
    ///
    /// Canonical message form:
    /// `"shell-extract::extract: medial-mask phase: no medial axis found — body '<name>' may be too degenerate for shell extraction (geometry fully solid or voxel resolution too coarse)"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_SHELL_NO_MEDIAL`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    ShellNoMedial,
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
    /// Origin: `crates/reify-expr/src/lib.rs` (`eval_solve_load_cases` emission
    /// site).
    ///
    /// Emitted as `Severity::Error` when `solve_load_cases` receives an empty
    /// `cases` list (`[]`). Multi-load-case analysis requires at least one
    /// `LoadCase`; single-case analysis should use `solve_elastic_static`.
    ///
    /// Canonical message form:
    /// `"Multi-load case analysis requires at least one LoadCase. Use solve_elastic_static for single-case analysis."`.
    ///
    /// Maps the PRD's illustrative `multi-load-case::empty-cases` code
    /// (v0.3.x multi-load-case FEA PRD task #10).
    MultiLoadEmptyCases,
    /// Origin: `crates/reify-expr/src/lib.rs` (`eval_solve_load_cases` emission
    /// site).
    ///
    /// Emitted as `Severity::Error` when two `LoadCase` values passed to a
    /// single `solve_load_cases` call share the same `name` field. Each load
    /// case must have a unique name so that downstream `linear_combine` weight
    /// maps can reference cases unambiguously.
    ///
    /// Canonical message form:
    /// `"Duplicate load case name: '<name>'. Each LoadCase in a single solve_load_cases call must have a unique name."`.
    ///
    /// Maps the PRD's illustrative `multi-load-case::duplicate-case-name` code
    /// (v0.3.x multi-load-case FEA PRD task #10).
    MultiLoadDuplicateCaseName,
    /// Origin: `crates/reify-stdlib/src/fea.rs` (`diagnose` classifier) +
    ///          `crates/reify-expr/src/lib.rs` (Undef-site emission).
    ///
    /// Emitted as `Severity::Error` when a `linear_combine` weights map
    /// references a case name that is not present in the `MultiCaseResult`
    /// being combined (typically a misspelled case name).
    ///
    /// Canonical message form:
    /// `"linear_combine: weights map references unknown case '<name>'. Available cases: [<list>]. Did you misspell the case name?"`.
    ///
    /// Maps the PRD's illustrative `multi-load-case::unknown-case-in-weights`
    /// code (v0.3.x multi-load-case FEA PRD task #10).
    MultiLoadUnknownCaseInWeights,
    /// Origin: `crates/reify-stdlib/src/fea.rs` (`diagnose` classifier) +
    ///          `crates/reify-expr/src/lib.rs` (Undef-site emission).
    ///
    /// Emitted as `Severity::Error` when two cases combined by
    /// `linear_combine` use incompatible meshes (different `mesh_size` or
    /// `element_order` in their `ElasticOptions`), detected structurally via a
    /// sampled-field grid/domain/codomain mismatch. Superposition requires
    /// matching mesh / element-order layouts.
    ///
    /// Canonical message form:
    /// `"linear_combine: cases '<name1>' and '<name2>' use incompatible meshes (different mesh_size or element_order in their ElasticOptions). Superposition requires matching mesh / element-order layouts. Re-solve with consistent options or compute envelopes instead."`.
    ///
    /// Maps the PRD's illustrative `multi-load-case::incompatible-meshes` code
    /// (v0.3.x multi-load-case FEA PRD task #10).
    MultiLoadIncompatibleMeshes,
    /// Origin: `crates/reify-stdlib/src/fea.rs` (`diagnose` classifier) +
    ///          `crates/reify-expr/src/lib.rs` (Undef-site emission).
    ///
    /// Emitted as `Severity::Error` when a `linear_combine` weights map is
    /// empty (`{}`). At least one weighted base case must be specified.
    ///
    /// Canonical message form:
    /// `"linear_combine: weights map is empty. Specify at least one weighted base case."`.
    ///
    /// Maps the PRD's illustrative `multi-load-case::empty-weights` code
    /// (v0.3.x multi-load-case FEA PRD task #10).
    MultiLoadEmptyWeights,
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
    /// Origin: `crates/reify-compiler/src/expr.rs`, BinOp Pow+Scalar branch.
    ///
    /// Emitted as `Severity::Error` when a dimensioned (`Scalar<Q>`) value is
    /// raised to an exponent that is NOT an integer literal.  PRD §4.3 requires
    /// an integer LITERAL for `Scalar<Q> ^ n → Scalar<Q^n>` because the
    /// dimension vector `Q^n` must be computed at compile time; a runtime value
    /// or a real-valued literal cannot provide this.
    ///
    /// Accepted exponent forms:
    ///   - `ExprKind::NumberLiteral { is_real: false, .. }` (positive integer literal)
    ///   - `ExprKind::UnOp { op: "-", operand: NumberLiteral { is_real: false, .. } }`
    ///     (negative integer literal — `^` binds tighter than unary `-`)
    ///
    /// Rejected exponent forms (all produce this code):
    ///   - Real literals (`is_real: true`), even when integer-valued (e.g. `2.0`)
    ///   - Identifier references or any other non-literal expression
    ///
    /// Canonical message form:
    ///   `"non-integer exponent on dimensioned value `T`; only integer-literal
    ///    exponents are allowed (use sqrt for roots)"`
    /// with a label `"exponent must be an integer literal"` on the exponent span.
    ///
    /// Per PRD §11.2 Q2: a dedicated code is minted rather than reusing the
    /// Add/Sub-specific `DimensionMismatch` code, whose semantics ("dimension
    /// mismatch in op: L vs R") do not fit this case.
    ///
    /// The PRD-prose mnemonic for this code is `E_NONINT_EXP_ON_DIMENSIONED`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    NonIntegerExponentOnDimensioned,
    /// Origin: `crates/reify-compiler/src/expr.rs` (BinOp::Mod compile site).
    ///
    /// Emitted as a `Severity::Error` when a `%` expression has at least one
    /// non-`Int` operand.  Both operands must be `Type::Int` for modulo to be
    /// well-typed; `Real`, dimensioned scalars, `Bool`, and any other shape are
    /// rejected.  The result type is poisoned to `Type::Error` (anti-cascade).
    ///
    /// Canonical message form:
    ///   `"modulo \`%\` requires Int operands, got \`L\` % \`R\`"`
    /// with a label `"operands must be Int"` on the expression span.
    ///
    /// Applies spec §5.1 "modulo is Int % Int -> Int ONLY".  A dedicated code is
    /// minted rather than reusing `DimensionMismatch` because `DimensionMismatch`
    /// semantics ("dimension mismatch in op: L vs R") do not fit `Real % Int`
    /// (same reasoning task-3805 used for `NonIntegerExponentOnDimensioned`).
    ///
    /// The PRD-prose mnemonic for this code is `E_MODULO_REQUIRES_INT`
    /// (severity convention: `E_*` → Error).
    ModuloRequiresInt,
    /// Origin: `crates/reify-eval/src/engine_eval.rs` (post-eval MassProperties
    /// PSD hook in the RBD-α dynamics foundation pass).
    ///
    /// Emitted as a `Severity::Error` in two situations, both replacing the
    /// cell with `Value::Undef` so downstream dynamics consumers never operate
    /// on a physically invalid or unresolvable inertia tensor:
    ///
    /// 1. The `inertia` field is present but cannot be parsed as a 3×3 numeric
    ///    matrix (wrong shape, non-numeric cell).  Canonical message form:
    ///    `"MassProperties '<name>': inertia field cannot be parsed as a 3×3 numeric matrix"`.
    ///
    /// 2. The symmetric part `(M + Mᵀ)/2` has a minimum eigenvalue below −tol,
    ///    i.e. the inertia tensor is not positive semi-definite.  Canonical
    ///    message form:
    ///    `"MassProperties '<name>': inertia tensor is not positive semi-definite (min eigenvalue ≈ <λ_min>)"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_DynamicsInertiaNotPSD`.
    /// Registered in task 3822 (RBD-α, PRD §dynamics).
    DynamicsInertiaNotPSD,
    /// Origin: `crates/reify-eval/src/dynamics_ops.rs` (`resolve_body_density`
    /// in the RBD-β `body_mass_props` density ladder, ambient-default-material
    /// task C).
    ///
    /// Emitted as a `Severity::Error` once per body when `body_mass_props`
    /// cannot resolve any density — the call supplies no explicit `density`
    /// argument AND the body carries no `Material` with a `density` field (incl.
    /// no `default Material = …` in scope, which would have been injected at
    /// compile time by the conformance checker). Unlike the former water-default
    /// advisory warning, this is a hard error: no density is available, so no
    /// physically meaningful mass properties can be computed. The returned
    /// `MassProperties` instance carries `Value::Undef` for all geometric fields
    /// (`mass`, `com`, `inertia`) — the same degrade shape as a rejected explicit
    /// density argument.
    ///
    /// Canonical message form:
    /// `"body_mass_props('<name>'): no density resolvable — pass an explicit \
    ///  density argument, give the body a Material with a density, or declare \
    ///  \`default Material = …\` in scope"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_DynamicsNoDensity`
    /// (severity convention: `E_*` → Error). Registered in task 4498
    /// (ambient-default-material C, PRD
    /// `docs/prds/v0_6/ambient-default-material.md` §3 decision 4).
    DynamicsNoDensity,
    /// Origin: `crates/reify-eval/src/dynamics_ops.rs`
    /// (`try_eval_body_mass_props` dispatch arity guard in the RBD-β stdlib-fn
    /// pass).
    ///
    /// Emitted as a `Severity::Error` when a recognised `body_mass_props(...)`
    /// call reaches the eval-layer dispatch with the wrong number of arguments
    /// — zero, or more than two — for the `body_mass_props(body, density?)`
    /// signature. No `MassProperties` is assembled; the cell is left at the
    /// `Value::Undef` produced by the pure `eval_expr` path. This is the
    /// eval-layer safety net for the case where the compiler's name-recognition
    /// path (`crates/reify-compiler/src/expr.rs`) assigns the `MassProperties`
    /// result type without an arity check, so a malformed-arity call would
    /// otherwise leave a `MassProperties`-typed cell silently holding `Undef`.
    /// The primary arity gate remains the compiler `body_mass_props(body,
    /// density?)` signature.
    ///
    /// Canonical message form:
    /// `"body_mass_props expects 1 or 2 arguments (body, density?), got <n>"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_DynamicsBodyMassPropsArity`
    /// (severity convention: `E_*` → Error). Registered in task 3829 (RBD-β).
    DynamicsBodyMassPropsArity,
    /// Origin: `crates/reify-stdlib/src/dynamics/eval.rs` (`diagnose` hook,
    /// wired into `reify-expr::emit_undef_builtin_diagnostics`).
    ///
    /// Emitted as a `Severity::Error` when an `inverse_dynamics` call returns
    /// `Value::Undef` because at least one body in the spanning tree has no
    /// resolvable mass — i.e. `body.solid` is neither a `MassProperties`
    /// `Value::StructureInstance` nor a real-geometry solid with density (the
    /// derived rung is a stub until task 3620). The diagnostic names the
    /// first unresolvable body.
    ///
    /// Canonical message form:
    /// `"inverse_dynamics: body '<id>' has no resolvable mass (no MassProperties on body.solid)"`.
    ///
    /// The PRD-prose mnemonic for this code is `E_DynamicsBodyMassUnresolved`.
    /// Registered in task 4278 (v0.3 flexures uniform-mass substrate).
    DynamicsBodyMassUnresolved,
    /// Origin: `crates/reify-stdlib/src/snapshot.rs` `diagnose` hook, wired
    /// into `reify-expr` as `emit_snapshot_diagnostics` (task 4471).
    ///
    /// Emitted as a `Severity::Warning` when a `center_of_mass` call falls
    /// back to the legacy density-weighted centroid because at least one
    /// snapshot body has no resolvable mass (neither an explicit
    /// `MassProperties` solid nor a build-baked `derived_mass_props` field)
    /// while at least one other body **does** carry resolvable mass (the
    /// mixed case). Pure-legacy snapshots (no body resolves) and
    /// all-resolved snapshots are silent.
    ///
    /// The diagnostic names the **first** unresolved body by its integer id.
    ///
    /// Canonical message form:
    /// `"center_of_mass: body '<id>' has no resolvable mass; falling back to
    ///   legacy density-weighted centroid (explicit point_mass/mass_properties
    ///   on other bodies ignored)"`.
    ///
    /// The PRD-prose mnemonic for this code is `W_SnapshotCenterOfMassDensityFallback`.
    /// Registered in task 4471 (v0.3 flexures snapshot center_of_mass rung).
    SnapshotCenterOfMassDensityFallback,
    /// Origin: `crates/reify-compiler/src/conformance` (assoc-fn satisfaction
    /// phase) and `crates/reify-compiler/src/trait_requirements.rs`.
    ///
    /// Canonical message form:
    /// `"trait '<Trait>' requires associated function '<fn>', but '<Structure>' does not provide it"`.
    ///
    /// Emitted as a `Severity::Error` when a structure declares conformance to a
    /// trait that has a bodyless required associated function
    /// (`RequirementKind::Fn`), the structure does not declare a `fn` member of
    /// that name, and the trait provides no default body (`DefaultKind::Fn`) for
    /// it. A single label is attached at the structure span naming the missing
    /// associated function.
    ///
    /// The PRD-prose mnemonic for this code is `E_TRAIT_FN_NOT_SATISFIED`
    /// (see `docs/prds/v0_6/trait-associated-functions.md` §5.4 / §8 Phase 3).
    TraitFnNotSatisfied,
    /// Origin: `crates/reify-compiler/src/conformance` (assoc-fn satisfaction
    /// phase, override check) and `crates/reify-compiler/src/trait_requirements.rs`
    /// (refinement signature-lock in `collect_all_requirements`).
    ///
    /// Canonical message form:
    /// `"associated function '<fn>' signature mismatch: trait requires <expected>, found <actual>"`
    /// (override case) or
    /// `"refining trait may not change inherited associated-function signature for '<fn>'"`
    /// (refinement case).
    ///
    /// Emitted as a `Severity::Error` in two situations, both PRD §5.4 / §8.8
    /// (associated-function signatures match exactly — self-ness, parameter
    /// types, and return type — with no subtyping):
    ///
    /// 1. A structure provides a `fn` of the required/default name but with a
    ///    different signature than the trait declares (override mismatch).
    /// 2. A refining trait re-declares an inherited associated function with a
    ///    different signature than the trait it refines (refinement
    ///    signature-lock).
    ///
    /// Distinct from [`TraitFnNotSatisfied`] (which covers the absent-fn case):
    /// here the function is present, just mis-typed. A dedicated code rather than
    /// reusing [`TypeMismatchForTraitMember`] gives the override-mismatch and
    /// refinement signature-lock tests an unambiguous signal.
    ///
    /// The PRD-prose mnemonic for this code is `E_TRAIT_FN_SIGNATURE_MISMATCH`.
    TraitFnSignatureMismatch,
    /// Origin: `crates/reify-compiler/src/conformance` and
    ///          `crates/reify-compiler/src/conformance/checker.rs`
    ///          (assoc-type satisfaction phase, check_phase_check_members_against_requirements).
    ///
    /// Canonical message form:
    /// `"trait '<Trait>' requires associated type '<Type>', but '<Structure>' does not bind it"`.
    ///
    /// Emitted as a `Severity::Error` when a structure declares conformance to a
    /// trait that has a required associated type (`RequirementKind::AssocType`),
    /// the structure does not declare a `type X = …` member binding it, and the
    /// trait provides no default (`DefaultKind::AssocType`) for it. A single label
    /// is attached at the structure span naming the missing associated type.
    ///
    /// The PRD-prose mnemonic for this code is `E_TRAIT_ASSOC_TYPE_NOT_BOUND`
    /// (see task 3972; trait-assoc-type iota-β).
    TraitAssocTypeNotBound,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs`
    ///          (assoc-type default conflict path in `collect_all_requirements`).
    ///
    /// Canonical message form:
    /// `"conflicting trait associated type for '<name>': trait '<A>' provides '<T1>', trait '<B>' provides '<T2>'"`.
    ///
    /// Emitted as a `Severity::Error` when two traits each provide a
    /// `DefaultKind::AssocType` default for the same associated-type name with
    /// different resolved types, and the conforming structure does not bind the
    /// name itself (which would suppress the conflict). Mirrors
    /// [`ConflictingTraitRequirements`] for param/let conflicts.
    ///
    /// The PRD-prose mnemonic for this code is `E_CONFLICTING_TRAIT_ASSOC_TYPE`
    /// (see task 3972; trait-assoc-type iota-β).
    ConflictingTraitAssocType,
    /// Origin: `crates/reify-compiler/src/type_resolution.rs`
    ///          (`resolve_qualified_assoc_type`, the qualified-assoc type-expr resolver).
    ///
    /// Canonical message form:
    /// `"ambiguous associated type '<Structure>::<Member>': declared by traits '<A>', '<B>'; \
    ///  qualify as '<Structure>::(<Trait>::<Member>)' to disambiguate"`.
    /// (Trait names are comma-joined; the phrasing is "qualify as".)
    ///
    /// Emitted as a `Severity::Error` when a bare qualified associated-type access
    /// `Base::Member` (a `TypeExprKind::QualifiedAssoc` with no `trait_name`) names a
    /// member that is declared by two or more of `Base`'s conformed traits, so the
    /// intended declaration is ambiguous. A single label is attached at the type-expr
    /// span suggesting the `Base::(Trait::Member)` paren disambiguator (FORK-G). The
    /// structure binds the associated type once, so the qualifier is
    /// disambiguation-only — every valid qualifier resolves to the same `Type`.
    ///
    /// Sibling of [`TraitAssocTypeNotBound`] / [`ConflictingTraitAssocType`] (the
    /// producer-side assoc-type diagnostics); this code is the consumer-side
    /// resolution diagnostic.
    ///
    /// The PRD-prose mnemonic for this code is `E_AMBIGUOUS_ASSOC_TYPE`
    /// (see task 3974; trait-assoc-type iota-ε).
    AmbiguousAssocType,
    /// Origin: `crates/reify-compiler/src/type_resolution.rs`
    /// (`check_applied_type_arg_bounds`, called from
    /// `phase_pending_bound_checks` in `entities_phase.rs` after all entities
    /// compile).
    ///
    /// Emitted as a `Severity::Error` when a structure type-annotation of the
    /// form `name<args…>` supplies a number of type arguments that does not
    /// match the declared arity of the named structure's type-param list.
    /// Covers both too-many-args and too-few-args, as well as zero declared
    /// type-params being given args (non-generic structure used with args).
    ///
    /// Canonical message form:
    ///   `"type '<name>' expects <N> type argument(s) but <M> were supplied"`
    /// with a label at the annotation span.
    ///
    /// Distinct from [`UnresolvedType`] (which fires when the name itself is
    /// unknown) and [`AmbiguousAssocType`] (associated-type resolution
    /// ambiguity). This code is ONLY emitted on the structure-member-annotation
    /// path (value_cells); sub-component and fn-call paths emit code-less
    /// diagnostics per PRD §7.3.
    ///
    /// PRD mnemonic: `E_TYPE_ARG_ARITY`.
    /// See `docs/prds/type-args-and-assoc-type-projection.md` §4.2, §9.
    TypeArgArity,
    /// Origin: `crates/reify-compiler/src/type_resolution.rs`
    /// (`check_applied_type_arg_bounds`, called from
    /// `phase_pending_bound_checks` in `entities_phase.rs` after all entities
    /// compile).
    ///
    /// Emitted as a `Severity::Error` when a type argument supplied to a
    /// generic structure (`name<arg>`) does not satisfy the declared bound on
    /// the corresponding type parameter (e.g. `Coupling<NotMotion>` when
    /// `Coupling<P: HasMotion>` requires `P` to conform to `HasMotion`).
    ///
    /// Canonical message form:
    ///   `"type argument '<arg>' for '<name>' does not satisfy bound '<Trait>'"`.
    /// with a label at the annotation span.
    ///
    /// Distinct from [`UnresolvedType`] / [`AmbiguousAssocType`]; those codes
    /// address name-resolution failures rather than bound violations.
    /// This code is ONLY emitted on the structure-member-annotation path
    /// (value_cells); sub-component and fn-call paths keep code-less
    /// diagnostics per PRD §7.3.
    ///
    /// PRD mnemonic: `E_TYPE_ARG_BOUND`.
    /// See `docs/prds/type-args-and-assoc-type-projection.md` §4.2, §9.
    TypeArgBound,
    /// Origin: `crates/reify-compiler/src/expr.rs` (BinOp::Pow + Scalar branch).
    ///
    /// Emitted as a `Severity::Error` when a dimensioned (`Scalar<Q>`) value is
    /// raised to an integer literal exponent whose value overflows `i8` (i.e. lies
    /// outside `[-128, 127]`).  PRD §4.3 requires the dimension vector `Q^n` to be
    /// computed at compile time, which requires the exponent to fit in the `i8`
    /// slot accepted by `DimensionVector::pow(i8)`.
    ///
    /// Distinct from [`NonIntegerExponentOnDimensioned`] (which rejects non-integer
    /// or non-literal exponents) and from `UnitResolveError::ExponentOutOfRange`
    /// (a unit-module error type whose uncoded diagnostic fires on the unit-literal
    /// path, e.g. `5mm^256` without spaces).  A dedicated code keeps the value-level
    /// path's coded-diagnostic convention and lets compile tests assert on `d.code`
    /// rather than on message substrings.
    ///
    /// Canonical message form:
    ///   `"exponent <n> is out of range for dimensioned `^`; must fit in i8 ([-128, 127])"`
    /// with a label `"exponent out of range"` on the exponent span.
    ///
    /// The result type is poisoned to `Type::Error` (anti-cascade), mirroring the
    /// adjacent [`NonIntegerExponentOnDimensioned`] branch.
    ///
    /// The PRD-prose mnemonic for this code is `E_EXPONENT_OUT_OF_RANGE`
    /// (severity convention: `E_*` → Error).
    ExponentOutOfRange,
    /// Origin: `crates/reify-compiler/src/expr.rs`
    /// (`ExprKind::FunctionCall` arm — semantic gate for VALUE-position `auto`).
    ///
    /// Canonical message form:
    /// `"auto is not allowed in a function-call argument (function '<name>'); \
    ///  to expose a free parameter, declare `param <name> = auto` at a binding site instead"`.
    ///
    /// Emitted as `Severity::Error` when an `ExprKind::Auto` node reaches a
    /// FUNCTION-call argument position.  Structure construction (`Bolt(length: auto)`)
    /// is explicitly exempt: named-arg `auto` at a construction site adopts
    /// determinacy-Auto on the field cell, which is resolved by downstream task ε;
    /// only non-structure callees trigger this gate.
    ///
    /// The result is poisoned to `Type::Error` (anti-cascade), with a label at
    /// the offending `auto` argument's span.  Only the first offending argument
    /// is reported per call site (subsequent `auto` args are suppressed to avoid
    /// multiplied noise).
    ///
    /// The PRD-prose mnemonic for this code is `E_AUTO_NOT_AT_BINDING_SITE`
    /// (see `docs/prds/auto-binding-site-positions.md` §"δ — function-call gate").
    ///
    /// **Distinction from `AutoTypeParam*` family.** The existing
    /// `AutoTypeParam*` codes (`AutoTypeParamPoolOverflow`, `AutoTypeParamNoCandidate`,
    /// etc.) all concern TYPE-POSITION `auto:` bindings resolved during phase-C
    /// auto-type-param resolution.  This code is VALUE-POSITION: it fires when
    /// the `auto` keyword appears as a VALUE argument to a function, not as a
    /// type-bound annotation.
    AutoNotAtBindingSite,
    /// Origin: `crates/reify-stdlib/src/flexures/diagnostics.rs::flexure_diagnose`
    /// (task 3871 — PRD `docs/prds/v0_3/compliant-joints.md` §5.3 "max stress
    /// at declared range endpoint" / §10.1 worked examples).
    ///
    /// Canonical message form:
    /// `"flexure surface stress <max_stress> exceeds material yield <yield> at the declared range endpoint (safety factor <sf>); narrow the operating range to ±<safe_angle>"`.
    ///
    /// Emitted as a `Severity::Warning` by the PRB-ctor success-path hook
    /// (`flexure_diagnose`, dispatched from reify-expr's `FunctionCall` arm)
    /// when a flexure's cached `FlexureCompliance.at_yield` is `true` — i.e. the
    /// peak bending stress at the user-declared operating-range endpoint reaches
    /// or exceeds the material yield stress (PRD §5.3). The constructor still
    /// returns a valid joint; the warning, not a poisoned `Undef`, is the
    /// user-facing signal that the declared range over-stresses the flexure.
    ///
    /// The PRD-prose mnemonic for this code is `W_FLEXURE_YIELDING`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FlexureYielding,
    /// Origin: `crates/reify-stdlib/src/flexures/diagnostics.rs::flexure_diagnose`
    /// (task 3871 — PRD `docs/prds/v0_3/compliant-joints.md` §5.3 / §1; the ±5°
    /// PRB small-deflection validity bound, Howell §5).
    ///
    /// Canonical message form:
    /// `"flexure declared operating range ±<declared> exceeds the ±5° PRB small-deflection validity bound; results beyond this angle require nonlinear FEA (see docs/prds/v0_3/compliant-joints.md §5.3)"`.
    ///
    /// Emitted as a `Severity::Warning` by the PRB-ctor success-path hook when
    /// the user-declared operating range is wider than the pseudo-rigid-body
    /// small-deflection validity bound (±5°). The PRB closed-form stiffness loses
    /// fidelity past this angle; the warning cites the 5° bound and the
    /// bookmarked nonlinear-FEA escalation path.
    ///
    /// The PRD-prose mnemonic for this code is `W_FLEXURE_PRB_OUT_OF_RANGE`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FlexurePrbOutOfRange,
    /// Origin: `crates/reify-stdlib/src/flexures/diagnostics.rs::flexure_diagnose`
    /// (task 3871 — PRD `docs/prds/v0_3/compliant-joints.md` §1 fatigue-advisory).
    ///
    /// Canonical message form:
    /// `"flexure constructed without a fatigue check; cyclic flexures should be checked against the material endurance limit (informational)"`.
    ///
    /// Emitted as a `Severity::Info` once per eval session (deduped per
    /// diagnostics sink at the reify-expr emission layer) the first time any PRB
    /// flexure constructor is evaluated. It is a standing advisory that the v0.3
    /// flexure surface does not perform fatigue / endurance-limit checking, not a
    /// per-flexure defect — hence once-per-session, Info severity.
    ///
    /// The PRD-prose mnemonic for this code is `W_FLEXURE_FATIGUE_CHECK_MISSING`
    /// (the `W_*` prefix is the PRD spelling; this code is emitted at
    /// `Severity::Info` per §1's "informational" qualifier — the advisory must
    /// not fail a build or be filtered out as a hard warning).
    FlexureFatigueCheckMissing,
    /// Origin: `crates/reify-stdlib/src/flexures/diagnostics.rs::flexure_diagnose`
    /// (task 3871 — PRD `docs/prds/v0_3/compliant-joints.md` §1 geometry
    /// validity: thickness < length, t < 2r for notches, PRB aspect ratio).
    ///
    /// Canonical message form:
    /// `"flexure geometry is degenerate: <reason> (e.g. thickness ≥ length, or notch thickness ≥ 2·radius); no valid pseudo-rigid-body joint can be constructed"`.
    ///
    /// Emitted as a `Severity::Error` by the PRB-ctor hook when a constructor
    /// returns `Value::Undef` AND the argument geometry is degenerate per §1
    /// (re-classified from args by `common::classify_geometry_invalid`). Other
    /// `Undef` causes (bad material, bad axis, wrong arity) do NOT emit this code
    /// — only geometry violations, mirroring how `stackup_diagnose` re-classifies
    /// on the `Undef` path.
    ///
    /// The PRD-prose mnemonic for this code is `E_FLEXURE_GEOMETRY_INVALID`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FlexureGeometryInvalid,
    /// Origin: `crates/reify-stdlib/src/flexures/diagnostics.rs::flexure_diagnose`
    /// (task 4547 — Disposition 5; PRD `docs/prds/v0_3/compliant-joints.md` §4.2
    /// `flexure_compliance(joint)` accessor).
    ///
    /// Canonical message form:
    /// `"W_FLEXURE_NON_JOINT_ARG: flexure_compliance() was called on a value that is not a flexure joint; the accessor returns a sentinel-zero compliance record, masking the misuse"`.
    ///
    /// Emitted as a `Severity::Warning` by the eval-time `flexure_diagnose`
    /// `__flexure_compliance_get` arm when the accessor's argument is NOT a joint
    /// `Value::Map` carrying the reserved hidden `__flexure_compliance` record
    /// (e.g. a bare `Length`). The DSL `flexure_compliance(joint: Length)`
    /// signature cannot distinguish a real PRB-ctor joint from any other `Length`
    /// at compile time, so the intrinsic silently yields a sentinel-zero record;
    /// this runtime warning surfaces that documented type-lie. A real joint
    /// argument emits nothing. Full static enforcement rides the future
    /// typed-joint work (out of scope here).
    ///
    /// The PRD-prose mnemonic for this code is `W_FLEXURE_NON_JOINT_ARG`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FlexureNonJointArg,
    /// Origin: `crates/reify-compiler/src/entity.rs::compile_entity`
    /// (objective-build site, task 4010 — PRD
    /// `docs/prds/v0_6/constraint-solver-completion.md` task ζ §3.3/§6.3
    /// boundary-sketch B3).
    ///
    /// Canonical message prefix: `"E_OBJECTIVE_CONFLICT: ..."`.
    ///
    /// Emitted as a `Severity::Error` when an entity's `ObjectiveSet` has
    /// `combination == WeightedSum`, more than one term, every term at default
    /// weight (1.0) and priority (0), and at least one pair of terms with
    /// **opposite sense** (`Minimize` vs `Maximize`) over **distinct
    /// expressions** (compared by `CompiledExpr.content_hash`). This is the
    /// PRD §6.3 "conflict without weighting = error" predicate.
    ///
    /// Correctly excluded cases:
    /// - Two same-sense default-weight terms (`minimize mass` + `minimize cost`)
    ///   — equal-weight sum (B2); see β's `objective_set_lowering` test.
    /// - A single objective.
    /// - Mixed-sense over the **same** expression (`minimize mass` + `maximize mass`)
    ///   — the "distinct expressions" qualifier.
    /// - Any `Lexicographic` combination (not yet source-reachable per §5).
    ///
    /// The three escapes named in the diagnostic message are:
    /// (1) assign non-default weights, (2) assign non-default priorities,
    /// (3) combine objectives into one expression before `minimize`/`maximize`.
    ///
    /// The PRD-prose mnemonic is `E_OBJECTIVE_CONFLICT`
    /// (severity convention: `E_*` → Error).
    ObjectiveConflict,
    /// Origin: `crates/reify-eval/src/engine_eval.rs::detect_scope_coupling`.
    ///
    /// Severity: Warning — detection-only; no automatic fixup is attempted.
    ///
    /// The PRD-prose mnemonic for this code is `W_SCOPE_COUPLING`
    /// (severity convention: `W_*` → Warning).
    ///
    /// Emitted when a bottom-up (leaf-first) per-scope auto-resolution walk is
    /// an approximation: an ALREADY-RESOLVED (frozen) scope's auto cell is read
    /// by a constraint or objective in a *different* scope that resolves LATER
    /// in the walk.  The diagnostic names the frozen scope (leaf), the later
    /// scope (reader), and the crossing `ValueCellId`.
    ///
    /// References: PRD `docs/prds/v0_6/constraint-solver-completion.md` task λ,
    /// §3.7 ("scope coupling"), §10.6 (detection-only boundary), and boundary
    /// sketch B11 (`reify check` prints `W_SCOPE_COUPLING`).
    ///
    /// **Detection-only**: coupling RESOLUTION (fixed-point iteration or
    /// re-ordering) is explicitly out of scope per PRD §10.  A future task may
    /// add resolution on top of this detection signal.
    ScopeCoupling,
    /// Origin: `crates/reify-eval/src/compute_targets/buckling.rs`
    /// (`solve_buckling_trampoline` option extractor — `buckling_unsupported_option_diagnostics`).
    ///
    /// Canonical message form:
    /// `"BucklingOptions.<param> = <value> is declared but not yet honored by the solver::buckling trampoline (the buckling kernel has no <param> input yet); solve falls back to the default <default>"`.
    ///
    /// Emitted as a `Severity::Warning` (PRD-prose mnemonic `W_BucklingOptionUnsupported`)
    /// when a declared-but-not-yet-honored `BucklingOptions` param (`mode`, `sigma`,
    /// `auto_dense`) is present AND set to a non-default value:
    ///   - `mode != "shift_invert"` (default from `solver_buckling.ri:84`)
    ///   - `sigma != 0.0`           (default from `solver_buckling.ri:85`)
    ///   - `auto_dense != true`     (default from `solver_buckling.ri:88`)
    ///
    /// One Warning is emitted per non-default unsupported param.  Absent fields
    /// and default values produce no diagnostic — robust to whether the eval
    /// pipeline materializes defaulted params or omits them.
    ///
    /// Firing on ANY non-default value (not just out-of-allowlist values) is more
    /// honest than allowlist validation because even a valid value like
    /// `mode: "dense"` is silently dropped today — the user deserves to know it
    /// has no effect.  The solve continues with kernel defaults (this is advisory,
    /// not an error).
    BucklingOptionUnsupported,
    /// Origin: `crates/reify-compiler/src/diagnostics.rs::dup_member_key_error`,
    /// wired into the keyed-sub pre-pass in
    /// `crates/reify-compiler/src/entity.rs` (`MemberDecl::Sub` arm).
    ///
    /// Canonical message form:
    /// `"E_DUP_MEMBER_KEY: duplicate keyed member key '<key>' in keyed sub '<sub>'"`.
    ///
    /// Emitted as an `Error` when two members of the same `Keyed<T>`
    /// sub-collection declare the same author-assigned String key. Keys must be
    /// unique within one keyed collection (keys are author-assigned; no
    /// auto-keys), so a duplicate is a compile-time identity collision. Carries
    /// two labels: the duplicate occurrence ("duplicate key defined here") and
    /// the first occurrence ("first defined here"), mirroring the duplicate
    /// port-name / duplicate meta-key pre-pass diagnostics.
    ///
    /// The PRD-prose mnemonic for this code is `E_DUP_MEMBER_KEY`
    /// (see `docs/prds/keyed-collection-identity.md` task β).
    DuplicateMemberKey,
    /// Origin: `crates/reify-compiler/src/expr.rs` — `ExprKind::FunctionCall`
    /// compile path (task 4197 α).
    ///
    /// Canonical message form:
    /// `"E_DETERMINACY_INTRINSIC_SCOPE: <name> is a purpose-body determinacy
    /// intrinsic and may only appear as a top-level constraint inside a purpose body"`.
    ///
    /// Emitted as a `Severity::Error` when `AllParamsDetermined` or
    /// `AllGeometryDetermined` is used outside a purpose body (or in a nested
    /// sub-expression position that reaches `compile_expr` without being desugared
    /// by `compile_purpose`). These names are reserved as compiler-sugar intrinsics;
    /// they are not user-callable functions. The intrinsics are valid ONLY as
    /// direct top-level `constraint` members of a purpose body, where
    /// `compile_purpose` rewrites them to a `forall … determined(…)` AST before
    /// calling `compile_expr`.
    ///
    /// Returns a non-cascading poison literal (`Value::Undef, Type::Error`) so
    /// downstream expressions do not emit spurious follow-on errors.
    ///
    /// See also: `DeterminacyIntrinsicArg` (E_DETERMINACY_INTRINSIC_ARG) for
    /// the bad-argument variant fired when the intrinsic IS in a purpose body
    /// but with an invalid argument.
    DeterminacyIntrinsicScope,
    /// Origin: `crates/reify-compiler/src/traits.rs::compile_purpose` (task 4197 α).
    ///
    /// Canonical message form:
    /// `"E_DETERMINACY_INTRINSIC_ARG: <name> expects exactly one purpose-parameter
    /// (entity reference) argument"`.
    ///
    /// Emitted as a `Severity::Error` when `AllParamsDetermined` or
    /// `AllGeometryDetermined` appears as a top-level `constraint` in a purpose
    /// body but with an invalid argument: wrong arity (0 or ≥2 args), a
    /// non-identifier argument (e.g. a literal or computed expression), or an
    /// identifier that is not a registered purpose parameter. The argument MUST be
    /// exactly one bare identifier that resolves to a purpose parameter via
    /// `scope.purpose_param_root`.
    ///
    /// Returns a non-cascading poison placeholder constraint so constraint indices
    /// remain stable and exactly one diagnostic is emitted (anti-cascade policy).
    ///
    /// See also: `DeterminacyIntrinsicScope` (E_DETERMINACY_INTRINSIC_SCOPE) for
    /// the out-of-scope variant fired when the intrinsic is used outside a purpose
    /// constraint position entirely.
    DeterminacyIntrinsicArg,
    /// Origin: `crates/reify-eval/src/engine_fixpoint.rs::run_unified_pass`
    /// (task 4357 δ; unified build-DAG Stage B Tarjan-SCC discriminator).
    ///
    /// Canonical message form (one diagnostic per strongly-connected component):
    /// `"evaluation cycle detected: [<member>, <member>, …]"`, where each
    /// member is rendered via [`crate::NodeId::describe`] along a deterministic
    /// ordered path (mirroring the legacy `detect_let_cycle` `[a, b, c]` shape).
    ///
    /// Emitted as a `Severity::Error` when the online Kahn worklist leaves a
    /// residue whose induced subgraph contains a true cycle (`|SCC| > 1`, or a
    /// singleton carrying a self-edge). Singleton-no-self-edge residue members
    /// are stranded-downstream and do NOT emit this code. Kind-agnostic: detects
    /// value↔value, geom↔constraint, realization↔realization (GeomRef::Sub) and
    /// any other cross-kind cycle over the edges α's trace map encodes.
    ///
    /// The PRD-prose mnemonic for this code is `E_EVAL_CYCLE`.
    EvalCycle,
    /// Origin: `crates/reify-eval/src/engine_fixpoint.rs::run_unified_pass`
    /// (task 4357 δ; geometry-backed-constraint-on-auto guard).
    ///
    /// Canonical message form:
    /// `"unresolved constraint: <constraint-describe> transitively depends on
    /// auto parameter(s) through geometry-backed inputs"`.
    ///
    /// Emitted as a `Severity::Error` when a constraint's transitive auto-read
    /// closure (its `realization_reads`, then each backing realization's
    /// `reads` + `realization_reads`, recursively) reaches an `auto` value cell
    /// (`ValueCellKind::is_auto`). The unified pass declines to solve that
    /// class. Independent of the cycle residue (a pure structural classifier
    /// over existing edges).
    ///
    /// The PRD-prose mnemonic for this code is `E_EVAL_UNRESOLVED`.
    EvalUnresolved,
    /// Origin: `crates/reify-eval/src/engine_constraints.rs::check_gdt_legality`
    /// (task 4475 β — GD&T zones β check-time legality diagnostics).
    ///
    /// Canonical message form:
    /// `"GD&T material modifier (MMC/LMC) is illegal for '<type_name>': this characteristic is RFS-only"`.
    ///
    /// Emitted as a `Severity::Error` when a callout instance (a `Value::StructureInstance`
    /// conforming to `GeometricTolerance`) carries `material_condition ∈ {MMC, LMC}` for a
    /// characteristic family that is RFS-only per ASME Y14.5-2018 — namely Form
    /// (`Flatness`, `Straightness`, `Circularity`, `Cylindricity`), Runout
    /// (`CircularRunout`, `TotalRunout`), and Profile (`ProfileOfSurface`,
    /// `ProfileOfLine`, `ProfileOfSurfaceRelated`, `ProfileOfLineRelated`).
    /// Orientation (`Parallelism`, `Perpendicularity`, `Angularity`) are FOS-eligible
    /// only when `zone_shape == Cylindrical`; when `zone_shape == Width` (the default)
    /// a modifier is also illegal and this code is emitted.
    ///
    /// The label is anchored at the ctor-let instantiation span (`ValueCellDecl.span`),
    /// which is the B7 "at the instantiation span" oracle.
    ///
    /// The PRD-prose mnemonic for this code is `E_GdtIllegalModifier`
    /// (severity convention: `E_*` → Error; see `docs/prds/v0_6/gdt-geometric-zones-and-containment.md` task β §11 Q3).
    GdtIllegalModifier,
    /// Origin: `crates/reify-eval/src/engine_constraints.rs::check_gdt_legality`
    /// (task 4475 β — GD&T zones β check-time legality diagnostics).
    ///
    /// Canonical message form:
    /// `"'<type_name>' was removed in ASME Y14.5-2018; use Position, ProfileOfSurface, or CircularRunout/TotalRunout instead"`.
    ///
    /// Emitted as a `Severity::Warning` (non-fatal) when a callout instance is of type
    /// `Concentricity` or `Symmetry`, both of which were removed from the standard in
    /// ASME Y14.5-2018. The warning fires unconditionally (independent of
    /// `material_condition`). `GdtIllegalModifier` is NOT additionally emitted for these
    /// types — the removal supersedes the modifier-legality question.
    ///
    /// The label is anchored at the ctor-let instantiation span (`ValueCellDecl.span`).
    ///
    /// The PRD-prose mnemonic for this code is `W_GdtRemoved2018`
    /// (severity convention: `W_*` → Warning; see `docs/prds/v0_6/gdt-geometric-zones-and-containment.md` task β §11 Q3).
    GdtRemoved2018,
    /// Origin: `crates/reify-compiler/src/expr.rs` (the `MemberAccess`
    /// datum-projection branch, geometric-relations β).
    ///
    /// Canonical message form:
    /// `"<type> has no projection '.<member>'"`, optionally followed by a
    /// `"; use .<member>"` redirect when an obvious alternative exists — e.g.
    /// `"Point3<Length> has no projection '.dir'"` (no redirect), but
    /// `"Plane has no projection '.dir'; use .normal"` (a plane's unique
    /// direction is its normal). The redirect hint is supplied by
    /// `datum_projection::datum_projection_unavailable_hint`.
    ///
    /// Emitted as a `Severity::Error` when a datum-projection member access
    /// (`.dir`/`.normal`/`.origin`/`.x`/`.y`/`.z`/`.xy_plane`) targets a receiver
    /// that has no such projection — e.g. `point.dir` (a `Point3` has no
    /// direction), or `plane.dir` (a `Plane` exposes `.normal`, not `.dir`). The
    /// access lowers to a poison literal (anti-cascade), so no further diagnostics
    /// fan out from the rejected member.
    ///
    /// Distinct from [`DatumProjectionAmbiguous`]: *unavailable* means the member
    /// does not exist on the receiver at all, whereas *ambiguous* means it could
    /// name several members and the author must pick one.
    ///
    /// The PRD-prose mnemonic for this code is `E_DATUM_PROJECTION_UNAVAILABLE`
    /// (severity convention: `E_*` → Error; see
    /// `docs/prds/v0_6/geometric-relations.md` §9 β).
    DatumProjectionUnavailable,
    /// Origin: `crates/reify-compiler/src/expr.rs` (the `MemberAccess`
    /// datum-projection branch, geometric-relations β).
    ///
    /// Canonical message form:
    /// `"ambiguous datum projection '.<member>' on <type>: it could be any of
    /// <members> — write one of those instead (e.g. write .<member>)"` (e.g.
    /// `"ambiguous datum projection '.dir' on Frame(3): it could be any of .x,
    /// .y, .z — write one of those instead (e.g. write .z)"`).
    ///
    /// Emitted as a `Severity::Error` when a *bare* datum projection could resolve
    /// to more than one member of the receiver — e.g. `frame.dir`/`frame.normal`
    /// on a `Frame(3)`, whose three basis directions `.x`/`.y`/`.z` are all
    /// candidates. The diagnostic names the disambiguating members; the access
    /// lowers to a poison literal (anti-cascade).
    ///
    /// Distinct from [`DatumProjectionUnavailable`]: *ambiguous* means the
    /// projection exists but is non-unique, whereas *unavailable* means it does
    /// not exist on the receiver at all.
    ///
    /// The PRD-prose mnemonic for this code is `E_DATUM_PROJECTION_AMBIGUOUS`
    /// (severity convention: `E_*` → Error; see
    /// `docs/prds/v0_6/geometric-relations.md` §9 β).
    DatumProjectionAmbiguous,
    /// Origin: `crates/reify-compiler/src/entity.rs` (the `MemberDecl::Relate`
    /// arm and the inline `SubDecl.relate_relations` check, geometric-relations δ).
    ///
    /// Canonical message form:
    /// `"relate member has type <T>, expected Relation"` — e.g. a Bool member
    /// (`relate { true }`) or a metric query (`relate { distance(p1, p2) }`,
    /// `Scalar<Length>`). A `relate { }` block — and its inline
    /// `sub … at … where { }` twin — accepts ONLY `Type::Relation` members
    /// (design §4/§7.3): a `drive` relation (`concentric`/`flush`/`offset`/…).
    ///
    /// Emitted as `Severity::Error` when a relate-block member's `result_type`
    /// is neither `Type::Relation` nor `Type::Error` (a `Type::Error` member is
    /// skipped — anti-cascade, so no second diagnostic piles onto an already-
    /// errored member). The 3-verb routing falls out of this single check with
    /// no name re-classification: a `check` verb types to `Bool` and a
    /// `derive`/`query` verb types to a metric, both failing the Relation check.
    ///
    /// The symmetric mirror is the constraint side rejecting `Type::Relation`
    /// (a Relation belongs in `relate {}`, not `constraint`; see the
    /// `MemberDecl::Constraint` arm).
    ///
    /// The PRD-prose mnemonic for this code is `E_RELATE_EXPECTS_RELATION`
    /// (severity convention: `E_*` → Error; see
    /// `docs/prds/v0_6/geometric-relations.md` §9 δ).
    RelateExpectsRelation,
    /// Origin: `crates/reify-compiler/src/conformance/mod.rs` (StructureRef nominal
    /// arg/default mismatch — task 4584).
    ///
    /// Emitted as `Severity::Error` in three sub-cases, differentiated by message text:
    /// 1. A constructor arg passed to a `Type::StructureRef` param has the wrong nominal
    ///    type (e.g. `ForcingTimeHistory(part: "beam", ...)` where `part : Part`).
    /// 2. A structure `param`'s default expression has the wrong nominal type for a
    ///    `Type::StructureRef`-typed cell (e.g. `param part : Part = "x"`).
    /// 3. A structure `param`'s default expression is not a geometry-producing expression
    ///    for a `Type::Geometry`-typed cell (e.g. `param g : Solid = 42`).
    ///
    /// Canonical message forms:
    /// - `"argument '<arg>' has type '<T>' but param '<p>' requires structure type '<S>'"` (sub-case 1/2)
    /// - `"param '<p>' has type 'Geometry' but its default expression has non-geometry type '<T>'"` (sub-case 3)
    ///
    /// One `DiagnosticCode` spans all three sub-cases per the established
    /// [`TypeNotConformingToTrait`] precedent (one code, message disambiguates).
    TypeNotConformingToStructureRef,
    /// Origin: `crates/reify-compiler/src/conformance/mod.rs` (Vector-param
    /// arg mismatch — task 4622).
    ///
    /// Emitted as a `Severity::Error` when a constructor arg passed to a
    /// `Type::Vector`-typed param is not vector-shaped (e.g. a bare scalar
    /// literal `1.0` passed where a `Vec3` / `Vector3<Length>` is required).
    ///
    /// Canonical message form:
    /// `"argument '<a>' has type '<T>' but param '<p>' requires vector type '<V>'"`
    ///
    /// Conformance is SHAPE-based (accepts any `Type::Vector { .. }` or
    /// `Type::Tensor { rank: 1, .. }`; rejects bare scalars, strings, bools,
    /// and other non-vector kinds), NOT `type_compatible`-based — the quantity
    /// slot on vectors is intentionally loose (see `ty.rs` Point/Vector
    /// quantity-slot convention), so a dimensionless `vec3(0,0,1)` arg is
    /// valid for a `Vector3<Length>` param.
    TypeNotConformingToVector,
    /// Origin: `crates/reify-eval/src/feature_datum.rs`
    /// (`feature_datum_projection`, geometric-relations ε).
    ///
    /// Emitted as `Severity::Error` at *resolve time* when a feature → datum
    /// projection (`feature.axis` / `.plane` / `.point` / `.dir`) cannot refine
    /// to a single datum: the realized feature's deduplicated
    /// `FeatureDatumBundle` carries either zero or several non-equivalent
    /// candidates for the requested projection target (e.g. `box.axis` →
    /// several non-coaxial edge axes). The projection evaluates to `Value::Undef`
    /// (the runtime analogue of β's poison literal) and the author must select a
    /// sub-feature to disambiguate.
    ///
    /// Canonical message form:
    /// `"ambiguous feature datum projection '.<member>': the feature carries <n>
    /// candidate <member> datums — select a sub-feature to disambiguate"`
    /// (`<n>` is the candidate count; the zero case reads "carries no <member>
    /// datum").
    ///
    /// This is the *resolve-time* (realized-geometry-dependent) sibling of the
    /// *compile-time* [`DatumProjectionAmbiguous`]: β's ambiguity is a static
    /// type-level non-uniqueness (`frame.dir`), whereas ε's depends on the dedup
    /// result of the realized geometry and so cannot be known at type-check time
    /// (design §7.2 — the ambiguous arm of the `Axis | Axis?` refinement). It is
    /// surfaced as a select-a-subfeature diagnostic rather than a static error or
    /// a new optional/union type.
    ///
    /// The PRD-prose mnemonic for this code is `E_FEATURE_DATUM_AMBIGUOUS`
    /// (severity convention: `E_*` → Error; see
    /// `docs/prds/v0_6/geometric-relations.md` §9 ε).
    FeatureDatumAmbiguous,

    // ── Geometric-joints diagnostics (task 4396 β) ───────────────────────

    /// Origin: `crates/reify-compiler/src/joint_self_check.rs` (the
    /// definition-time DOF self-check, geometric-joints β).
    ///
    /// Emitted as `Severity::Error` at *definition time* (before any solve) when
    /// a `joint NAME(datums) with <declared free DOF> = <relation body>`
    /// definition's declared free DOF does NOT match the body's geometric
    /// residual — by COUNT or by KIND. A mechanism nominally has 6 spatial DOF
    /// (3 rotational + 3 translational, PRD §7.1.2); each body relation removes a
    /// curated `(rot, trans)` codimension split, leaving a residual
    /// `(3 − Σrot, 3 − Σtrans)`. The declared DOF fields contribute their own
    /// kinds (`Angle` → rotational, `Length` → translational, `Orientation` → 3
    /// rotational). The law is exact-integer equality of the two `(rot, trans)`
    /// pairs — no tolerance (design §7.1; PRD §12 G6 numeric-floor is N/A).
    ///
    /// Canonical message form:
    /// `"declared <Nr> rotational [+ <Nt> translational] free DOF, but the
    /// relation leaves <Rr> rot + <Rt> trans; add a constraint or declare
    /// <field>: <Type>"` — naming the count and/or kind disagreement and a
    /// geometric remedy (add a constraint to remove a residual freedom, or
    /// declare the missing DOF field with its `Angle`/`Length`/`Orientation`
    /// type). An empty body (residual `(3, 3)`) cannot equal any sane declared
    /// multiset, so it surfaces here naturally rather than via a bespoke
    /// empty-body code.
    ///
    /// The PRD-prose mnemonic for this code is `E_JOINT_DOF_MISMATCH`
    /// (severity convention: `E_*` → Error; see
    /// `docs/prds/v0_6/geometric-joints.md` §7.1).
    JointDofMismatch,

    // ── FEA failure-mode diagnostics (task 2929) ─────────────────────────
    //
    // Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    // (task 2929 — FEA diagnostic mapping for common failure modes).
    // Conversion: `fea_diagnostic_to_core` in the same file.
    //
    // Severity convention (matches FeaFailure::is_error()):
    //   W_FEA_* → Warning (advisory, solve still returns a result)
    //   E_FEA_* → Error   (degenerate / unresolvable, solve aborts)

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when no user-specified supports are provided
    /// (empty `supports` list); the fixed-cantilever trampoline auto-clamps
    /// the root face so the solve still returns an ElasticResult.
    ///
    /// The PRD-prose mnemonic for this code is `W_FEA_UNDER_CONSTRAINED`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaUnderConstrained,

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when one or more elements have near-zero volume
    /// (degenerate mesh); the stiffness matrix is singular and the solve
    /// cannot proceed. Returns `ComputeOutcome::Failed`.
    ///
    /// The PRD-prose mnemonic for this code is `E_FEA_SINGULAR_STIFFNESS`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaSingularStiffness,

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when the CG solver reaches its iteration limit
    /// without converging. The result is returned but may be inaccurate.
    ///
    /// The PRD-prose mnemonic for this code is `W_FEA_NON_CONVERGENCE`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaNonConvergence,

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when no loads are applied to the model (all
    /// applied forces are zero). The solve produces a trivial all-zero
    /// result.
    ///
    /// The PRD-prose mnemonic for this code is `W_FEA_NO_LOADS`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaNoLoads,

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when a load selector targets an interior node
    /// rather than a boundary face.
    ///
    /// **Not yet emitted.** The trampoline receives already-resolved
    /// `Load`/`Support` `StructureInstance` values (selector resolution is
    /// upstream); detection wiring is deferred to a follow-up task.  The
    /// variant and its `fea_diagnostic_to_core` arm are reserved so downstream
    /// tooling can match on the typed code the moment wiring lands.
    ///
    /// The PRD-prose mnemonic for this code is `E_FEA_LOAD_ON_INTERIOR`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaLoadOnInterior,

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when a selector matches no geometry nodes.
    ///
    /// **Not yet emitted.** The trampoline receives already-resolved
    /// `Load`/`Support` `StructureInstance` values (selector resolution is
    /// upstream); detection wiring is deferred to a follow-up task.  The
    /// variant and its `fea_diagnostic_to_core` arm are reserved so downstream
    /// tooling can match on the typed code the moment wiring lands.
    ///
    /// The PRD-prose mnemonic for this code is `E_FEA_SELECTOR_NO_MATCH`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaSelectorNoMatch,

    /// Origin: `crates/reify-eval/src/compute_targets/fea_diagnostics.rs`
    /// (task 2929). Emitted when the body bounding-box aspect ratio exceeds
    /// the thin-body threshold (~10); P1 solid elements perform poorly for
    /// very thin bodies. Advisory only — the solve still runs.
    ///
    /// The PRD-prose mnemonic for this code is `W_FEA_THIN_BODY`
    /// (severity convention: `W_*` → Warning, `E_*` → Error).
    FeaThinBody,


    /// Origin: `crates/reify-compiler/src/compile_builder/entities_phase.rs`
    /// (ambient-default collection pre-pass — ambient-default-material task B).
    ///
    /// Emitted as a `Severity::Error` when two `default <TypeName> = <expr>`
    /// declarations name the same type within the same scope (file top-level,
    /// or a single `purpose` body). Two declarations for one type in one scope
    /// is an ambiguity; the first declaration is retained as the table entry.
    ///
    /// Two labels accompany the error, mirroring [`DuplicateMemberKey`]: the
    /// duplicate occurrence (`"duplicate default declared here"`) and the first
    /// occurrence (`"first defined here"`).
    ///
    /// The PRD-prose mnemonic for this code is `E_DUP_AMBIENT_DEFAULT`
    /// (see `docs/prds/ambient-default-material.md` §7 invariant (ii)).
    DuplicateAmbientDefault,

    /// Origin: `crates/reify-compiler/src/compile_builder/entities_phase.rs`
    /// (ambient-default collection pre-pass — ambient-default-material task B).
    ///
    /// Emitted as a `Severity::Error` when the value expression of a
    /// `default <TypeName> = <expr>` declaration does not
    /// `implicitly_converts_to` the named type. Attributed ONCE at the
    /// declaration span (DD4), not at the use sites the default would have
    /// filled, so the designer sees one clear error at the source of the
    /// mistake (e.g. `default Material = 5mm`).
    ///
    /// A single label accompanies the error at the declaration span.
    ///
    /// The PRD-prose mnemonic for this code is `E_AMBIENT_DEFAULT_TYPE_MISMATCH`
    /// (see `docs/prds/ambient-default-material.md` §7).
    AmbientDefaultTypeMismatch,

    /// Origin: `crates/reify-expr/src/lib.rs` op/builtin contract-failure reason
    /// sink — γ (task 4323, PRD `docs/prds/v0_6/undef-self-describing.md` §4.3/§8.2).
    ///
    /// Emitted when an op or builtin returns `Value::Undef` with **all inputs
    /// determined** (i.e. this is a genuine domain/contract failure, not a
    /// propagated undef). Arithmetic and math-builtin domain failures
    /// (sqrt domain, div/mod-by-zero, pow non-finite, dimension mismatch, Point+Point)
    /// emit no pre-existing `DiagnosticCode`, so a new generic code is minted.
    ///
    /// PRD-prose mnemonic: `E_OP_CONTRACT`.
    /// Minting rationale: `DiagnosticCode` is `#[non_exhaustive]` with no exhaustive
    /// match-on-self; a single generic code is honest for v1 — finer per-op codes are
    /// a follow-up. Follows the `SelectorKindMismatch`/`ArgTypeMismatch` minting
    /// precedent; `DiagnosticCode` is `#[non_exhaustive]` so adding one variant is
    /// non-breaking for downstream consumers.
    OpContractViolation,

    /// Origin: `crates/reify-expr/src/lib.rs` — `eval_generate_dispatch`, the
    /// `n < 0` branch of the free-function `generate(n, |i| …)` combinator
    /// (task 3994, structural-query ζ; PRD
    /// `docs/prds/v0_6/structural-query-traversal.md` §2.3).
    ///
    /// Emitted at EVAL time when `generate`'s count argument is a negative `Int`
    /// (e.g. `generate(-1, f)`, or `generate(k, f)` where `k` resolves to a
    /// negative value). A negative count is a runtime VALUE concern: the literal
    /// `-1` types as `Type::Int` (UnOp::Neg over Int) and so passes the
    /// compile-time `ExpectedArg::Int` count check — a *non-integer* count is the
    /// separate compile-time `ArgTypeMismatch`.
    ///
    /// PRD-prose mnemonic: `E_GENERATE_NEGATIVE_COUNT`.
    /// Minting rationale: `DiagnosticCode` is `#[non_exhaustive]` with no exhaustive
    /// match-on-self, so adding one variant is non-breaking for downstream
    /// consumers (follows the `OpContractViolation` / `ArgTypeMismatch` precedent).
    GenerateNegativeCount,

    /// Origin: `crates/reify-compiler/src/expr.rs` — `MemberAccess` handler,
    /// `Type::TypeParam` branch (task 4596).
    ///
    /// Canonical message forms (see `expr.rs`, `MemberAccess` handler):
    ///
    /// - When the type parameter has bound trait(s):
    ///   `"type parameter '<param>' (bound: <trait1>, <trait2>) has no member \
    ///    '<member>': the bound trait does not declare '<member>'"`
    /// - When the type parameter has NO bounds:
    ///   `"type parameter '<param>' (bound: (no bounds on type parameter \
    ///    '<param>')) has no member '<member>': the bound trait does not \
    ///    declare '<member>'"`
    ///
    /// Emitted as `Severity::Error` (anti-cascade: one diagnostic + poison
    /// literal via `make_poison_literal`) when a member-access expression
    /// `<param>.<member>` is compiled against an unresolved `Type::TypeParam`
    /// receiver and no bound trait on that type parameter declares `<member>`
    /// as a `RequirementKind::Param` or `RequirementKind::Let` member.
    ///
    /// Sub-cases that all produce this code:
    ///   1. The type parameter has NO bounds (`T` with no `: Trait`).
    ///   2. A bound trait name is absent from the scope's `trait_member_types`
    ///      (should not happen in a well-formed compilation unit, but handled
    ///      defensively).
    ///   3. The bound trait(s) exist but none declares `<member>`.
    ///
    /// Soundness contract: the branch NEVER returns a node whose `result_type`
    /// is `Type::TypeParam` and NEVER synthesizes a permissive placeholder type
    /// (e.g. `Type::dimensionless_scalar()`), so no unsound substitution can
    /// propagate through a trait-contract violation.
    ///
    /// The PRD-prose mnemonic for this code is
    /// `E_TYPE_PARAM_MEMBER_NOT_IN_BOUND` (see
    /// `docs/prds/v0_3/auto-type-param-resolution-completion.md` §4596).
    TypeParamMemberNotInBound,

    /// Origin: `crates/reify-compiler/src/compile_builder/reserved_name_lint.rs`.
    ///
    /// Emitted as a `Warning` when a user-declared `enum`, `structure`, `occurrence`,
    /// or `trait` declaration uses a name that is also resolvable by the builtin type
    /// resolver (`resolve_type_name`). The builtin type still wins in type-annotation
    /// position; this warning exists to alert the author that the user declaration is
    /// silently shadowed.
    ///
    /// Canonical message form:
    /// `"<kind> '<name>' shadows a builtin type name; the builtin takes precedence in type position"`
    ///
    /// One label accompanies the warning at the user declaration's span:
    /// `"shadows a builtin type name"`.
    ///
    /// Builtin names covered: the datum types (`Direction`, `Axis`, `Plane`, `Frame`),
    /// scalar primitives (`Bool`, `Int`, `Real`, `String`), the selector family
    /// (`Selector`, `FaceSelector`, `EdgeSelector`, `BodySelector`), geometry/solid types
    /// (`Geometry`, `Solid`, `DatumRef`), `Dimensionless`, and every named physical
    /// dimension (`Length`, `Mass`, `Force`, `Energy`, `Area`, `Volume`, `Angle`, …).
    ///
    /// The PRD-prose mnemonic for this code is `W_RESERVED_TYPE_NAME`.
    ReservedTypeName,

    /// Origin: `crates/reify-compiler/src/expr.rs` (BinOp::Eq/Ne/Lt/Le/Gt/Ge compile site).
    ///
    /// Emitted as a `Severity::Error` when a relational operator (any of `==`, `!=`,
    /// `<`, `<=`, `>`, `>=`) is applied to an operand whose type is not acceptable for
    /// that operator family:
    ///
    /// - ORDER ops (`<`, `<=`, `>`, `>=`): operand must be `Type::Int` or
    ///   `Type::Scalar { .. }` (`is_orderable_scalar`).
    /// - EQUALITY ops (`==`, `!=`): operand must be `Type::Bool`, `Type::Int`,
    ///   `Type::String`, `Type::Scalar { .. }`, or `Type::Enum(..)` (`is_equatable_kind`).
    ///
    /// Aggregate/structural kinds (`Tensor`, `Matrix`, `Vector`, `Point`, `List`, …)
    /// are rejected for ALL six operators — they produce `Value::Undef` at runtime,
    /// making any comparison silently indeterminate.
    ///
    /// When the offending operand is `Type::Tensor { .. }` or `Type::Matrix { .. }`,
    /// the message includes a fixit suggestion ("reduce to a scalar first, e.g.
    /// `eigenvalues(x)[0]` or `trace(x)`") and the candidates list is populated via
    /// [`Diagnostic::with_candidates`] for machine-readable IDE quick-fix support.
    ///
    /// Gradualism: operands typed `Type::Error` (poison) or `Type::TypeParam(_)`
    /// (unresolved auto/generic) pass through without emitting this code (anti-cascade).
    ///
    /// The result type is NOT poisoned — comparison ops return `Type::Bool` even on
    /// operand-kind errors (mirrors the Implies guard; avoids cascade noise in
    /// enclosing `constraint` expressions).
    ///
    /// Canonical message form (order op, left operand bad):
    ///   `"comparison \`<\` left operand must be a scalar or Int, got \`Tensor<2,3,Length>\`"`
    /// Canonical message form (equality op, left operand bad):
    ///   `"comparison \`==\` left operand is not a comparable kind, got \`Tensor<2,3,Length>\`; reduce to a scalar first, e.g. \`eigenvalues(x)[0]\` or \`trace(x)\`"`
    /// with a label `"not a comparable kind"` on the expression span.
    ///
    /// The PRD-prose mnemonic for this code is `E_CmpOperandKind`
    /// (severity convention: `E_*` → Error; see task 4490 type-hygiene α).
    CmpOperandKind,
    /// Origin: `crates/reify-compiler/src/expr.rs` (BinOp::And/Or/Implies compile site).
    ///
    /// Emitted as a `Severity::Error` when a logical operator (`and`, `or`, `implies`)
    /// is applied to an operand whose type is not `Type::Bool`.  All three logical
    /// operators require `Bool` operands; non-Bool values produce `Value::Undef` at
    /// runtime (Kleene three-valued logic: `Undef and false = false` / `Undef or true = true`,
    /// but `5 and flag` is an authoring mistake that always Undefs when `5` stays non-Bool).
    ///
    /// Generalizes the previously-uncoded Implies Bool guard (task 3921) to `And` and
    /// `Or` uniformly.  The Kleene RUNTIME eval (`eval_and`, `eval_or`, `eval_implies`
    /// in `reify-expr`) is NOT changed — only the compile-time diagnostic is added.
    ///
    /// Gradualism: operands typed `Type::Error` (poison) or `Type::TypeParam(_)`
    /// (unresolved auto/generic) pass through without emitting this code (anti-cascade).
    ///
    /// The result type is NOT poisoned — logical ops return `Type::Bool` even on
    /// operand errors (mirrors the prior Implies behavior).
    ///
    /// Canonical message form (left operand bad):
    ///   `"and left operand must be Bool, got \`Int\`"`
    /// with a label `"expected Bool here"` on the offending operand span.
    ///
    /// The PRD-prose mnemonic for this code is `E_LogicalRequiresBool`
    /// (severity convention: `E_*` → Error; see task 4490 type-hygiene α).
    LogicalOperandNotBool,
    /// Origin: `crates/reify-compiler/src/expr.rs` (the `COLLECTION_AGGREGATION_MEMBERS`
    /// wrong-receiver arms: `.sum` on a non-`List` receiver, or `.keys`/`.values` on a
    /// non-`Map` receiver; ds-sentinel L4, task #4649).
    ///
    /// Canonical message form:
    /// - `.sum`: `"'.sum' requires a List receiver, but got <type>"` with label `"wrong receiver type for aggregation"`.
    /// - `.keys`/`.values`: `"'.keys' requires a Map receiver, but got <type>"` (same label).
    ///
    /// Emitted as a `Severity::Error` when `.sum`, `.keys`, or `.values` is applied to a
    /// receiver that is not a `List` (for `.sum`) or a `Map` (for `.keys`/`.values`).  The
    /// access lowers into a `MethodCall` node whose `result_type` is `Type::Error` (poison,
    /// anti-cascade), so no further diagnostics fan out from the rejected aggregation.  The
    /// node shape (`CompiledExpr::method_call`) is unchanged from the non-error arms so eval
    /// behaviour is unaffected — the module simply carries an error and `reify check` reports it.
    ///
    /// Distinct from a missing struct member (the `:3445` / `:3385` sibling) and from a
    /// datum-projection error ([`DatumProjectionUnavailable`]).  The incoming-poison
    /// short-circuit at the top of the aggregation branch (`if compiled_obj.result_type.is_error()`
    /// `{ return propagate_poison(); }`) guarantees this code fires at most once per
    /// wrong-receiver site and never double-fires on an already-poisoned receiver.
    ///
    /// The PRD-prose mnemonic for this code is `E_AGGREGATION_RECEIVER_NOT_COLLECTION`
    /// (severity convention: `E_*` → Error; see `docs/prds/dimensionless-scalar-sentinel-stampout.md`
    /// §3 Tier-4 / §5 D6).
    AggregationReceiverNotCollection,
    /// Origin: `crates/reify-compiler/src/expr.rs` (two sibling "has no member" sites:
    /// the SIR-α entity-scope StructureRef member-access branch at `:3432`, and the
    /// purpose-subject member-access guard at `:3374`; ds-sentinel L4, task #4649).
    ///
    /// Canonical message form:
    ///   `"structure '<name>' has no member '<member>'"` with label `"unknown member"`.
    ///
    /// Emitted as a `Severity::Error` when a member access is attempted on a concrete
    /// `StructureRef`-typed value and the named member does not appear in the structure
    /// definition's `value_cells`, `ports`, or `sub_components`.  The access lowers to a
    /// poison literal (anti-cascade, `Type::Error`).  `TraitObject` receivers and structs
    /// not present in `template_registry` keep the existing permissive
    /// `dimensionless_scalar()` fallback — a static type is not knowable for those.
    ///
    /// Distinct from [`AggregationReceiverNotCollection`] (wrong receiver type for
    /// aggregation methods) and from [`DatumProjectionUnavailable`] (geometric datum
    /// projection failure).
    ///
    /// The PRD-prose mnemonic for this code is `E_STRUCTURE_MEMBER_NOT_FOUND`
    /// (severity convention: `E_*` → Error; see `docs/prds/dimensionless-scalar-sentinel-stampout.md`
    /// §3 Tier-4 / §5 D6).
    StructureMemberNotFound,

    /// Origin: `crates/reify-eval/src/cache.rs::CacheStore::write_intermediate`
    /// (task 3584 θ — GR-038 B6 PROGRESSIVE invariant cache-write guard).
    ///
    /// Emitted as a `Severity::Warning` when a node whose effective [`NodeTraits`]
    /// (resolved via `NodeTraitsMap::resolve`) lacks the `PROGRESSIVE` flag writes
    /// `Freshness::Intermediate` via the guarded deliberate-emission entry
    /// `CacheStore::write_intermediate`. This is a **soft invariant** (PRD §12 Q-5):
    /// the write always proceeds (to avoid dropping partial results), debug builds
    /// `debug_assert!`-panic instead of emitting, and only release builds emit this
    /// code and return `Some(Diagnostic)`.
    ///
    /// Canonical message form:
    /// `"node '<node>' wrote Freshness::Intermediate without the PROGRESSIVE trait"`
    ///
    /// `PROGRESSIVE` is the positive permit: tagging a node with `NodeTraits::PROGRESSIVE`
    /// (via `CacheStore::node_traits_mut().set_instance(node, NodeTraits::PROGRESSIVE)`)
    /// suppresses this diagnostic and allows deliberate progressive emission. The guard
    /// does NOT apply to the unguarded derivation/propagation path (`set_freshness`),
    /// which legitimately writes `Intermediate` to downstream Value cells.
    ///
    /// The PRD-prose mnemonic for this code is `W_PROGRESSIVE_INVARIANT_VIOLATED`
    /// (severity convention: `W_*` → Warning; see
    /// `docs/prds/v0_3/node-traits-unification.md` §5 B6 / §9 T7 / §12 Q-5).
    ProgressiveInvariantViolated,

    /// Origin: `crates/reify-core/src/diagnostics.rs::hex_wedge_mesh_diagnostic`
    /// (task #2992, PRD `docs/prds/v0_3/hex-wedge-meshing.md` task #11).
    ///
    /// Emitted as `Severity::Info` (never upgraded) when a swept body is
    /// successfully promoted to hex/wedge elements.
    ///
    /// Canonical message form:
    /// `"Body <body_label> meshed as <hex_count> hex / <wedge_count> wedge"`.
    ///
    /// The PRD-prose mnemonic for this code is `hex_wedge_promoted`.
    /// This is a success notice — `require_hex_wedge=true` does NOT upgrade it.
    HexWedgePromoted,

    /// Origin: `crates/reify-core/src/diagnostics.rs::hex_wedge_mesh_diagnostic`
    /// (task #2992, PRD `docs/prds/v0_3/hex-wedge-meshing.md` task #11).
    ///
    /// Emitted when a swept body has post-sweep modifications (finishing
    /// operations) that pure-sweep Phase A cannot promote, causing a fall-back
    /// to tetrahedral meshing.  Phase B (PRD task #14, axial-finishing
    /// operations) will eventually handle this case.
    ///
    /// Severity: `Info` by default; upgraded to `Error` when
    /// `require_hex_wedge=true` (the diagnostic code is preserved across
    /// the upgrade so downstream tooling can match the cause independently
    /// of severity).
    ///
    /// The PRD-prose mnemonic for this code is `hex_wedge_phase_a_finishing_ops`.
    HexWedgePhaseAFinishingOps,

    /// Origin: `crates/reify-core/src/diagnostics.rs::hex_wedge_mesh_diagnostic`
    /// (task #2992, PRD `docs/prds/v0_3/hex-wedge-meshing.md` task #11).
    ///
    /// Emitted when the swept-body geometry classifier rejects the body as
    /// a valid sweep (e.g. non-planar profile, non-translational axis),
    /// causing a fall-back to tetrahedral meshing.
    ///
    /// Severity: `Info` by default; upgraded to `Error` when
    /// `require_hex_wedge=true` (the diagnostic code is preserved across
    /// the upgrade).
    ///
    /// The PRD-prose mnemonic for this code is `hex_wedge_invalid_sweep_geometry`.
    HexWedgeInvalidSweepGeometry,

    /// Origin: `crates/reify-core/src/diagnostics.rs::hex_wedge_mesh_diagnostic`
    /// (task #2992, PRD `docs/prds/v0_3/hex-wedge-meshing.md` task #11).
    ///
    /// Emitted when the 2-D profile mesher fails for a swept body, causing
    /// a fall-back to tetrahedral meshing.
    ///
    /// Severity: `Info` by default; upgraded to `Error` when
    /// `require_hex_wedge=true` (the diagnostic code is preserved across
    /// the upgrade).
    ///
    /// The PRD-prose mnemonic for this code is `hex_wedge_2d_mesh_failure`.
    HexWedge2dMeshFailure,

    /// Origin: `crates/reify-core/src/diagnostics.rs::hex_wedge_mesh_diagnostic`
    /// (task #2992, PRD `docs/prds/v0_3/hex-wedge-meshing.md` task #11).
    ///
    /// Emitted when the `force_tet` debug flag suppresses hex/wedge promotion,
    /// forcing tetrahedral meshing regardless of sweep eligibility.
    ///
    /// Severity: always `Info` and upgrade-exempt (the `force_tet` and
    /// `require_hex_wedge` flags are mutually exclusive by stdlib constraint,
    /// so `require_hex_wedge=true` can never co-occur with this diagnostic at
    /// a real call site).  The PRD "debug" tier is realized as `Severity::Info`
    /// because `reify_core::Severity` has no `Debug` variant; adding one would
    /// require cross-cutting changes well outside this task's scope.
    ///
    /// The PRD-prose mnemonic for this code is `hex_wedge_force_tet`.
    HexWedgeForceTet,
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

// ---------------------------------------------------------------------------
// Hex/wedge swept-mesh diagnostic mapping (task #2992)
// ---------------------------------------------------------------------------

/// Cause of a hex/wedge swept-mesh outcome — success or one of four fall-back
/// reasons — produced by the volume-mesh dispatcher and consumed by
/// [`hex_wedge_mesh_diagnostic`] to build a typed [`Diagnostic`].
///
/// Each variant maps to a distinct [`DiagnosticCode`] (PRD task #11 in
/// `docs/prds/v0_3/hex-wedge-meshing.md`).
///
/// # Dead-code note
///
/// This enum and [`hex_wedge_mesh_diagnostic`] are tested but not yet wired
/// into the dispatcher (`dispatch_volume_mesh` in `reify-eval/src/engine_build.rs`)
/// — that wiring is blocked on VolumeMesh realization (task #2947), mirroring
/// the accepted `#[allow(dead_code)]` status of `p2_substitution_diagnostic`
/// (task #2991).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexWedgeMeshOutcome {
    /// Swept body successfully promoted to hex/wedge elements.
    ///
    /// `hex_count` and `wedge_count` are the element counts in the resulting
    /// mesh; both may be zero for degenerate cases (though this is unusual).
    Promoted {
        /// Number of hexahedral elements in the resulting mesh.
        hex_count: usize,
        /// Number of wedge (prism) elements in the resulting mesh.
        wedge_count: usize,
    },
    /// The swept body has post-sweep finishing operations that Phase A cannot
    /// promote; falls back to tetrahedral meshing.
    ///
    /// Phase B (PRD task #14, axial-finishing operations) will eventually
    /// handle this case.
    PhaseAFinishingOps,
    /// The body geometry is not a valid swept shape (e.g. non-planar profile or
    /// non-translational sweep axis); falls back to tetrahedral meshing.
    InvalidSweepGeometry,
    /// The 2-D profile mesher failed for this body; falls back to tetrahedral
    /// meshing.
    Mesh2dFailure,
    /// The `force_tet` debug flag suppressed hex/wedge promotion; tetrahedral
    /// meshing is used regardless of sweep eligibility.
    ///
    /// `force_tet` and `require_hex_wedge` are mutually exclusive by stdlib
    /// constraint, so this variant is upgrade-exempt under `require_hex_wedge`.
    ForceTet,
}

/// Map a hex/wedge mesh outcome to a typed [`Diagnostic`] with the appropriate
/// [`DiagnosticCode`] and [`Severity`].
///
/// # Severity policy (PRD task #11, `docs/prds/v0_3/hex-wedge-meshing.md`)
///
/// | Outcome               | Default severity | `require_hex_wedge=true` |
/// |-----------------------|-----------------|--------------------------|
/// | `Promoted`            | `Info`          | `Info` (never upgraded)  |
/// | `PhaseAFinishingOps`  | `Info`          | `Error`                  |
/// | `InvalidSweepGeometry`| `Info`          | `Error`                  |
/// | `Mesh2dFailure`       | `Info`          | `Error`                  |
/// | `ForceTet`            | `Info`          | `Info` (upgrade-exempt)  |
///
/// The [`DiagnosticCode`] is **preserved** across the `Info → Error` upgrade so
/// downstream tooling (e.g. the validation task #2993) can match the cause
/// independently of whether `require_hex_wedge` promoted it to an error.
///
/// # Wiring note
///
/// This function is `#[allow(dead_code)]` pending the live wiring of
/// `dispatch_volume_mesh` (blocked on task #2947).  The future dispatcher will
/// construct a `HexWedgeMeshOutcome` from its internal state and call this fn
/// to obtain the diagnostic to push into `BuildResult.diagnostics`.
///
/// # Placement note
///
/// Unlike the sibling `p2_substitution_diagnostic` (in
/// `reify-eval/src/engine_build.rs` next to its dispatcher), this function and
/// `HexWedgeMeshOutcome` live here in `reify-core` because their inputs are
/// pure primitives (`usize`, `bool`, `&str`) with no `reify-eval`/`reify-solver`
/// dependency.  The future task #2947 call site in `dispatch_volume_mesh`
/// should reach into `reify_core::diagnostics::{HexWedgeMeshOutcome,
/// hex_wedge_mesh_diagnostic}` rather than duplicating the mapping there.
#[allow(dead_code)]
pub fn hex_wedge_mesh_diagnostic(
    outcome: &HexWedgeMeshOutcome,
    require_hex_wedge: bool,
    body_label: &str,
) -> Diagnostic {
    // Invariant guard: force_tet=true and require_hex_wedge=true are mutually
    // exclusive by stdlib constraint.  A ForceTet outcome therefore must never
    // arrive with require_hex_wedge=true.  The guard fires only in debug builds,
    // so the production code path remains unchanged; it surfaces the violation
    // at the call site (task #2947) rather than relying solely on the upstream
    // stdlib enforcement that is not yet wired.
    debug_assert!(
        !(require_hex_wedge && matches!(outcome, HexWedgeMeshOutcome::ForceTet)),
        "invariant violated: force_tet and require_hex_wedge are mutually exclusive; \
         a ForceTet outcome must never be paired with require_hex_wedge=true"
    );

    // Helper shared by the three genuine fall-back causes (PhaseAFinishingOps,
    // InvalidSweepGeometry, Mesh2dFailure).  Centralises the severity-upgrade
    // rule so that changing it in one place automatically applies to all three.
    // Promoted and ForceTet are upgrade-exempt and do NOT use this helper.
    fn fallback(msg: String, require: bool) -> Diagnostic {
        if require { Diagnostic::error(msg) } else { Diagnostic::info(msg) }
    }

    match outcome {
        HexWedgeMeshOutcome::Promoted { hex_count, wedge_count } => {
            // Success path — always Info regardless of require_hex_wedge.
            Diagnostic::info(format!(
                "Body {body_label} meshed as {hex_count} hex / {wedge_count} wedge"
            ))
            .with_code(DiagnosticCode::HexWedgePromoted)
        }
        HexWedgeMeshOutcome::PhaseAFinishingOps => {
            let msg = format!(
                "Body {body_label} has post-sweep finishing operations that Phase A \
                 cannot promote to hex/wedge; falling back to tetrahedral meshing. \
                 Phase B (PRD task #14, axial-finishing operations) will eventually \
                 handle this case."
            );
            fallback(msg, require_hex_wedge)
                .with_code(DiagnosticCode::HexWedgePhaseAFinishingOps)
        }
        HexWedgeMeshOutcome::InvalidSweepGeometry => {
            let msg = format!(
                "Body {body_label} is not a valid swept shape (invalid sweep geometry); \
                 falling back to tetrahedral meshing."
            );
            fallback(msg, require_hex_wedge)
                .with_code(DiagnosticCode::HexWedgeInvalidSweepGeometry)
        }
        HexWedgeMeshOutcome::Mesh2dFailure => {
            let msg = format!(
                "Body {body_label}: 2-D profile meshing failed; \
                 falling back to tetrahedral meshing."
            );
            fallback(msg, require_hex_wedge)
                .with_code(DiagnosticCode::HexWedge2dMeshFailure)
        }
        HexWedgeMeshOutcome::ForceTet => {
            // force_tet is upgrade-exempt; always Info regardless of require_hex_wedge.
            Diagnostic::info(format!(
                "Body {body_label}: hex/wedge promotion suppressed by force_tet debug flag; \
                 using tetrahedral meshing."
            ))
            .with_code(DiagnosticCode::HexWedgeForceTet)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticCode, SourceSpan};

    /// Task 4357 δ (step-3): the two additive diagnostic codes emitted by
    /// `engine_fixpoint::run_unified_pass` — `EvalCycle` (E_EVAL_CYCLE) and
    /// `EvalUnresolved` (E_EVAL_UNRESOLVED) — must exist, be distinct, and be
    /// attachable via `Diagnostic::error(..).with_code(..)` with the code
    /// reading back.
    ///
    /// RED until step-4 adds the variants.
    #[test]
    fn eval_cycle_and_unresolved_codes_exist_and_attach() {
        // Exist + distinct.
        assert_ne!(DiagnosticCode::EvalCycle, DiagnosticCode::EvalUnresolved);

        // Attachable via the builder; code reads back.
        let cyc = Diagnostic::error("cycle").with_code(DiagnosticCode::EvalCycle);
        assert_eq!(cyc.code, Some(DiagnosticCode::EvalCycle));
        let unr = Diagnostic::error("unresolved").with_code(DiagnosticCode::EvalUnresolved);
        assert_eq!(unr.code, Some(DiagnosticCode::EvalUnresolved));
    }

    /// Task 4357 δ (step-3): the additive codes serialize to their PascalCase
    /// wire identifiers under the `serde` feature (matching the enum's
    /// `rename_all = "PascalCase"`), so downstream tooling matches stable
    /// strings rather than message substrings.
    ///
    /// RED until step-4 adds the variants.
    #[cfg(feature = "serde")]
    #[test]
    fn eval_codes_serialize_to_pascalcase_wire_strings() {
        assert_eq!(
            serde_json::to_value(DiagnosticCode::EvalCycle).unwrap(),
            serde_json::Value::String("EvalCycle".to_owned())
        );
        assert_eq!(
            serde_json::to_value(DiagnosticCode::EvalUnresolved).unwrap(),
            serde_json::Value::String("EvalUnresolved".to_owned())
        );
    }

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

    // --- GenerateNegativeCount tests (task 3994, structural-query ζ) ---
    // Pairs with the eval-time producer `push_eval_error(.., GenerateNegativeCount)`
    // in `crates/reify-expr/src/lib.rs` (`eval_generate_dispatch`, the `n < 0`
    // branch): `generate(n, |i| …)` with a negative count.

    /// `DiagnosticCode::GenerateNegativeCount` round-trips through
    /// `Diagnostic::error(...).with_code(...)` (mirrors the GeometryUnbounded shape
    /// so a future enum reorganisation that drops it is caught here).
    #[test]
    fn diagnostic_code_generate_negative_count_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::GenerateNegativeCount);
        assert_eq!(d.code, Some(DiagnosticCode::GenerateNegativeCount));
        assert_eq!(
            format!("{:?}", DiagnosticCode::GenerateNegativeCount),
            "GenerateNegativeCount"
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::GenerateNegativeCount` serializes
    /// as `"GenerateNegativeCount"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_generate_negative_count_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::GenerateNegativeCount).unwrap();
        assert_eq!(s, "\"GenerateNegativeCount\"");
    }

    // --- GeometryProfileRequired tests (geometry-primitive-constructors task α) ---
    // Pairs with the `emit_geometry_profile_required` producer in
    // `crates/reify-compiler/src/conformance/mod.rs`, called by the profile-consumer
    // arms in `crates/reify-compiler/src/geometry.rs` (extrude/revolve/loft/sweep/pipe…).

    /// `DiagnosticCode::GeometryProfileRequired` round-trips through
    /// `Diagnostic::error(...).with_code(...)` (mirrors the GeometryUnbounded
    /// shape so a future enum reorganisation that drops it is caught here).
    #[test]
    fn diagnostic_code_geometry_profile_required_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::GeometryProfileRequired);
        assert_eq!(d.code, Some(DiagnosticCode::GeometryProfileRequired));
        assert_eq!(
            format!("{:?}", DiagnosticCode::GeometryProfileRequired),
            "GeometryProfileRequired"
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::GeometryProfileRequired` serializes
    /// as `"GeometryProfileRequired"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_geometry_profile_required_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::GeometryProfileRequired).unwrap();
        assert_eq!(s, "\"GeometryProfileRequired\"");
    }

    // --- EmptyEdgeSelection tests (geom-modify curated-fillet task 3205) ---
    // Pairs with the anti-zero-edges guard in the eval ModifyKind::Fillet 3-arg
    // arm: a present (3-arg) edge selector that resolves to an empty vector emits
    // a blocking diagnostic with this code instead of silently filleting all edges.

    /// `DiagnosticCode::EmptyEdgeSelection` round-trips through
    /// `Diagnostic::error(...).with_code(...)` (mirrors the GeometryUnbounded
    /// shape so a future enum reorganisation that drops it is caught here).
    #[test]
    fn diagnostic_code_empty_edge_selection_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::EmptyEdgeSelection);
        assert_eq!(d.code, Some(DiagnosticCode::EmptyEdgeSelection));
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

    // --- FieldSamplesNotGrid tests (task 4221 — E_FIELD_SAMPLES_NOT_GRID) ---
    // Pairs with `eval_from_samples` in `crates/reify-expr/src/lib.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and severity tests are added here.

    /// `DiagnosticCode::FieldSamplesNotGrid` round-trips through
    /// `Diagnostic::error(...).with_code(...)` carrying both the expected
    /// `Severity::Error` and `Some(DiagnosticCode::FieldSamplesNotGrid)`.
    /// Pins the error-severity contract for E_FIELD_SAMPLES_NOT_GRID.
    #[test]
    fn field_samples_not_grid_diagnostic_code_is_constructible() {
        use super::Severity;
        let d = Diagnostic::error("not a 1-D regular grid").with_code(DiagnosticCode::FieldSamplesNotGrid);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::FieldSamplesNotGrid));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::FieldSamplesNotGrid`
    /// serializes as `"FieldSamplesNotGrid"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_field_samples_not_grid_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::FieldSamplesNotGrid).unwrap();
        assert_eq!(s, "\"FieldSamplesNotGrid\"");
    }

    // --- InterpMethodUnsupported tests (task 4221 — E_INTERP_METHOD_UNSUPPORTED) ---
    // Pairs with `eval_from_samples` in `crates/reify-expr/src/lib.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and severity tests are added here.

    /// `DiagnosticCode::InterpMethodUnsupported` round-trips through
    /// `Diagnostic::error(...).with_code(...)` carrying both the expected
    /// `Severity::Error` and `Some(DiagnosticCode::InterpMethodUnsupported)`.
    /// Pins the error-severity contract for E_INTERP_METHOD_UNSUPPORTED.
    #[test]
    fn interp_method_unsupported_diagnostic_code_is_constructible() {
        use super::Severity;
        let d = Diagnostic::error("interpolation method 'RBF' is not supported by from_samples")
            .with_code(DiagnosticCode::InterpMethodUnsupported);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::InterpMethodUnsupported));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::InterpMethodUnsupported`
    /// serializes as `"InterpMethodUnsupported"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_interp_method_unsupported_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::InterpMethodUnsupported).unwrap();
        assert_eq!(s, "\"InterpMethodUnsupported\"");
    }

    // --- RelateExpectsRelation tests (task 4384 δ — E_RELATE_EXPECTS_RELATION) ---
    // Pairs with the `MemberDecl::Relate` arm + `SubDecl.relate_relations` check
    // in `crates/reify-compiler/src/entity.rs`. Variant-agnostic
    // Copy/Clone/PartialEq/Eq/Hash/Debug derives are already covered by
    // `diagnostic_code_derives` above; only the variant-specific round-trip and
    // severity tests are added here.

    /// `DiagnosticCode::RelateExpectsRelation` round-trips through
    /// `Diagnostic::error(...).with_code(...)` carrying both the expected
    /// `Severity::Error` and `Some(DiagnosticCode::RelateExpectsRelation)`.
    /// Pins the error-severity contract for E_RELATE_EXPECTS_RELATION.
    #[test]
    fn relate_expects_relation_diagnostic_code_is_constructible() {
        use super::Severity;
        let d = Diagnostic::error("relate member has type Bool, expected Relation")
            .with_code(DiagnosticCode::RelateExpectsRelation);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::RelateExpectsRelation));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::RelateExpectsRelation`
    /// serializes as `"RelateExpectsRelation"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_relate_expects_relation_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::RelateExpectsRelation).unwrap();
        assert_eq!(s, "\"RelateExpectsRelation\"");
    }

    // --- JointDofMismatch tests (task 4396 β — E_JOINT_DOF_MISMATCH) ---
    // Pairs with the definition-time DOF self-check in
    // `crates/reify-compiler/src/joint_self_check.rs` (geometric-joints β).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::JointDofMismatch` round-trips through
    /// `Diagnostic::error(...).with_code(...)` carrying both the expected
    /// `Severity::Error` and `Some(DiagnosticCode::JointDofMismatch)`.
    /// Pins the error-severity contract for E_JOINT_DOF_MISMATCH.
    #[test]
    fn joint_dof_mismatch_diagnostic_code_is_constructible() {
        use super::Severity;
        let d = Diagnostic::error(
            "declared 1 rotational free DOF, but the relation leaves 1 rot + 1 trans; \
             add a constraint or declare travel: Length",
        )
        .with_code(DiagnosticCode::JointDofMismatch);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::JointDofMismatch));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::JointDofMismatch`
    /// serializes as `"JointDofMismatch"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_joint_dof_mismatch_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::JointDofMismatch).unwrap();
        assert_eq!(s, "\"JointDofMismatch\"");
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

    // --- AmbiguousAssocType tests (task 3974 — E_AMBIGUOUS_ASSOC_TYPE) ---
    // Pairs with `resolve_qualified_assoc_type` in
    // `crates/reify-compiler/src/type_resolution.rs` (qualified-assoc resolver).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip, severity, and serde wire-format tests are added here.

    /// `DiagnosticCode::AmbiguousAssocType` round-trips through
    /// `Diagnostic::error(...).with_code(...)` carrying both the expected
    /// `Severity::Error` and `Some(DiagnosticCode::AmbiguousAssocType)`.
    /// Pins the error-severity contract and variant existence for a bare
    /// qualified associated-type access ambiguous across two conformed traits
    /// (`Beam::Material` where two conformed traits each declare `Material`).
    #[test]
    fn diagnostic_code_ambiguous_assoc_type_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::error("x").with_code(DiagnosticCode::AmbiguousAssocType);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::AmbiguousAssocType));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::AmbiguousAssocType`
    /// serializes as `"AmbiguousAssocType"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_ambiguous_assoc_type_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::AmbiguousAssocType).unwrap();
        assert_eq!(s, "\"AmbiguousAssocType\"");
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

    // --- AutoTypeParamBoundedInfeasible tests (task 4434 — E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE) ---
    // Pairs with the joint-recheck emitter in
    // `crates/reify-compiler/src/auto_type_param.rs::emit_fallback_warning_and_delegate_to_bfs`.
    // The variant is registered in `crates/reify-core/src/diagnostics.rs` (not
    // reify-ir) alongside the other AutoTypeParam* siblings, per the design decision
    // recorded in task 4434's plan (reify-ir/src/diagnostics.rs does not exist).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // serde wire-form round-trip is added here to lock the LSP/MCP contract.

    /// `DiagnosticCode::AutoTypeParamBoundedInfeasible` round-trips through
    /// serde under `feature = "serde"`: the wire form is the PascalCase
    /// string `"AutoTypeParamBoundedInfeasible"`, and deserializing that
    /// string back yields the original variant.  Pins both directions of the
    /// LSP/MCP wire contract for the γ BFS-fallback joint-recheck hard error.
    ///
    /// Emitted (as `Severity::Error`) by
    /// `emit_fallback_warning_and_delegate_to_bfs` when BFS returns a complete
    /// assignment that the joint-recheck finds infeasible (any Violated
    /// constraint after seeding the full joint ValueMap).  Produces NO
    /// substitution (PRD §6.2 step 4 / mnemonic E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE).
    #[cfg(feature = "serde")]
    #[test]
    fn auto_type_param_bounded_infeasible_round_trips_via_serde() {
        let s =
            serde_json::to_string(&DiagnosticCode::AutoTypeParamBoundedInfeasible).unwrap();
        assert_eq!(
            s, "\"AutoTypeParamBoundedInfeasible\"",
            "serde wire form must equal PascalCase identifier"
        );
        let back: DiagnosticCode = serde_json::from_str(&s).unwrap();
        assert_eq!(
            back,
            DiagnosticCode::AutoTypeParamBoundedInfeasible,
            "deserialize must round-trip back to AutoTypeParamBoundedInfeasible"
        );
    }

    // --- AutoTypeParamCandidateNotConstructible tests (task 4435 — E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE) ---
    // Pairs with the synthesis guard in
    // `crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs`.
    // The variant is registered in `crates/reify-core/src/diagnostics.rs`
    // alongside the other AutoTypeParam* siblings per the task 4435 DIAGNOSTIC
    // HOME CORRECTION.  Variant-agnostic derives are covered by
    // `diagnostic_code_derives`; only the variant-specific serde wire-form
    // round-trip is added here to lock the LSP/MCP contract.

    /// `DiagnosticCode::AutoTypeParamCandidateNotConstructible` round-trips
    /// through serde under `feature = "serde"`: the wire form is the PascalCase
    /// string `"AutoTypeParamCandidateNotConstructible"`, and deserializing that
    /// string back yields the original variant.  Pins both directions of the
    /// LSP/MCP wire contract for the δ constructibility guard.
    ///
    /// Emitted (as `Severity::Error`) by the monomorph-build pass when a
    /// resolved candidate has ≥1 required (non-defaulted) Param cell, making
    /// it impossible to synthesize a zero-arg StructureInstanceCtor default
    /// (mnemonic E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE).
    #[cfg(feature = "serde")]
    #[test]
    fn auto_type_param_candidate_not_constructible_round_trips_via_serde() {
        let s = serde_json::to_string(
            &DiagnosticCode::AutoTypeParamCandidateNotConstructible,
        )
        .unwrap();
        assert_eq!(
            s, "\"AutoTypeParamCandidateNotConstructible\"",
            "serde wire form must equal PascalCase identifier"
        );
        let back: DiagnosticCode = serde_json::from_str(&s).unwrap();
        assert_eq!(
            back,
            DiagnosticCode::AutoTypeParamCandidateNotConstructible,
            "deserialize must round-trip back to AutoTypeParamCandidateNotConstructible"
        );
    }

    // --- AutoTypeParamConstraintUnevaluated tests (task 4616 — W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED) ---
    // Pairs with the Gap-C honesty emit helper in
    // `crates/reify-compiler/src/auto_type_param.rs::emit_unevaluated_constraint_warnings`.
    // The variant is registered in `crates/reify-core/src/diagnostics.rs`
    // alongside the other AutoTypeParam* siblings per task 4616's plan.
    // Variant-agnostic derives are covered by `diagnostic_code_derives`; only
    // the variant-specific serde wire-form round-trip is added here to lock the
    // LSP/MCP contract.

    /// `DiagnosticCode::AutoTypeParamConstraintUnevaluated` round-trips through
    /// serde under `feature = "serde"`: the wire form is the PascalCase string
    /// `"AutoTypeParamConstraintUnevaluated"`, and deserializing that string
    /// back yields the original variant. Pins both directions of the LSP/MCP
    /// wire contract for the Gap-C honesty warning
    /// (mnemonic W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED).
    ///
    /// Emitted (as `Severity::Warning`) by `emit_unevaluated_constraint_warnings`
    /// when a template-side auto: resolution constraint references a cell whose
    /// default expression is non-literal (computed) and was therefore skipped
    /// by the literal-only seeder `seed_template_literal_params`.
    #[cfg(feature = "serde")]
    #[test]
    fn auto_type_param_constraint_unevaluated_round_trips_via_serde() {
        let s = serde_json::to_string(
            &DiagnosticCode::AutoTypeParamConstraintUnevaluated,
        )
        .unwrap();
        assert_eq!(
            s,
            "\"AutoTypeParamConstraintUnevaluated\"",
            "serde wire form must equal PascalCase identifier"
        );
        let back: DiagnosticCode = serde_json::from_str(&s).unwrap();
        assert_eq!(
            back,
            DiagnosticCode::AutoTypeParamConstraintUnevaluated,
            "deserialize must round-trip back to AutoTypeParamConstraintUnevaluated"
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

    // --- FieldImportFailed tests (task 3576 step-8) ---

    /// `DiagnosticCode::FieldImportFailed` round-trips through
    /// `Diagnostic::error(...).with_code(...)` and Debug-prints as
    /// `"FieldImportFailed"`. Shape mirrors
    /// `diagnostic_code_geometry_unbounded_with_code_round_trips`.
    #[test]
    fn diagnostic_code_field_import_failed_with_code_round_trips() {
        let d = Diagnostic::error("field 'x': failed to import VDB file: not found")
            .with_code(DiagnosticCode::FieldImportFailed);
        assert_eq!(d.code, Some(DiagnosticCode::FieldImportFailed));
        assert_eq!(
            format!("{:?}", DiagnosticCode::FieldImportFailed),
            "FieldImportFailed"
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::FieldImportFailed` serializes
    /// as `"FieldImportFailed"` (PascalCase).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_field_import_failed_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::FieldImportFailed).unwrap();
        assert_eq!(s, "\"FieldImportFailed\"");
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

    // --- ObjectiveConflict tests (task 4010 — E_OBJECTIVE_CONFLICT) ---
    // Pairs with the conflict detector in
    // `crates/reify-compiler/src/entity.rs::compile_entity` (objective-build site).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::ObjectiveConflict` round-trips through
    /// `Diagnostic::error(...).with_code(...)` and reports
    /// `Some(DiagnosticCode::ObjectiveConflict)`.
    /// Shape mirrors `diagnostic_code_unresolved_type_with_code_round_trips`;
    /// a future enum reorganisation that drops `ObjectiveConflict` is caught here.
    #[test]
    fn diagnostic_code_objective_conflict_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::ObjectiveConflict);
        assert_eq!(d.code, Some(DiagnosticCode::ObjectiveConflict));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::ObjectiveConflict` serializes as
    /// `"ObjectiveConflict"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_objective_conflict_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::ObjectiveConflict).unwrap();
        assert_eq!(s, "\"ObjectiveConflict\"");
    }

    // --- Flexure DiagnosticCode tests (task 3871) ---

    /// The five §5.3 / §1 / §4.2 flexure codes round-trip through
    /// `Diagnostic::<sev>(...).with_code(...)` at their PRD-assigned severities:
    /// `FlexureYielding` / `FlexurePrbOutOfRange` / `FlexureNonJointArg` → Warning,
    /// `FlexureGeometryInvalid` → Error, `FlexureFatigueCheckMissing` → Info.
    /// Pins per-variant severity +
    /// variant-existence so an enum reorganisation that drops or re-tiers one of
    /// the flexure codes is caught at the reify-core layer. Mirrors
    /// `diagnostic_code_multi_kernel_variants_with_code_round_trip`.
    #[test]
    fn diagnostic_code_flexure_variants_with_code_round_trip() {
        use super::Severity;
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::FlexureYielding);
        assert_eq!(d.severity, Severity::Warning, "FlexureYielding is a Warning");
        assert_eq!(d.code, Some(DiagnosticCode::FlexureYielding));

        let d = Diagnostic::warning("x").with_code(DiagnosticCode::FlexurePrbOutOfRange);
        assert_eq!(
            d.severity,
            Severity::Warning,
            "FlexurePrbOutOfRange is a Warning"
        );
        assert_eq!(d.code, Some(DiagnosticCode::FlexurePrbOutOfRange));

        let d = Diagnostic::error("x").with_code(DiagnosticCode::FlexureGeometryInvalid);
        assert_eq!(
            d.severity,
            Severity::Error,
            "FlexureGeometryInvalid is an Error"
        );
        assert_eq!(d.code, Some(DiagnosticCode::FlexureGeometryInvalid));

        let d = Diagnostic::info("x").with_code(DiagnosticCode::FlexureFatigueCheckMissing);
        assert_eq!(
            d.severity,
            Severity::Info,
            "FlexureFatigueCheckMissing is Info (advisory)"
        );
        assert_eq!(d.code, Some(DiagnosticCode::FlexureFatigueCheckMissing));

        let d = Diagnostic::warning("x").with_code(DiagnosticCode::FlexureNonJointArg);
        assert_eq!(d.severity, Severity::Warning, "FlexureNonJointArg is a Warning");
        assert_eq!(d.code, Some(DiagnosticCode::FlexureNonJointArg));
    }

    /// Under `feature = "serde"`, each flexure code serializes to its PascalCase
    /// wire string (from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_flexure_variants_serde_pascal_case() {
        let cases = [
            (DiagnosticCode::FlexureYielding, "\"FlexureYielding\""),
            (
                DiagnosticCode::FlexurePrbOutOfRange,
                "\"FlexurePrbOutOfRange\"",
            ),
            (
                DiagnosticCode::FlexureFatigueCheckMissing,
                "\"FlexureFatigueCheckMissing\"",
            ),
            (
                DiagnosticCode::FlexureGeometryInvalid,
                "\"FlexureGeometryInvalid\"",
            ),
            (
                DiagnosticCode::FlexureNonJointArg,
                "\"FlexureNonJointArg\"",
            ),
        ];
        for (code, expected) in cases {
            let s = serde_json::to_string(&code).unwrap();
            assert_eq!(s, expected, "serde mismatch for {code:?}");
        }
    }

    // --- ScopeCoupling tests (task 4020 — W_SCOPE_COUPLING, PRD §3.7/§10.6, B11) ---
    // Pairs with `detect_scope_coupling` in `crates/reify-eval/src/engine_eval.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; the tests below pin the
    // per-variant round-trip, Debug string, and serde wire format.

    /// `DiagnosticCode::ScopeCoupling` round-trips through
    /// `Diagnostic::warning(...).with_code(...)`.
    /// Shape mirrors `diagnostic_code_shadowing_with_code_round_trips`.
    /// A future enum reorganisation that drops `ScopeCoupling` is caught here.
    /// The serde wire-format is independently pinned by
    /// `diagnostic_code_scope_coupling_serde_pascal_case` below.
    #[test]
    fn diagnostic_code_scope_coupling_with_code_round_trips() {
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::ScopeCoupling);
        assert_eq!(d.code, Some(DiagnosticCode::ScopeCoupling));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::ScopeCoupling` serializes as
    /// `"ScopeCoupling"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_scope_coupling_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::ScopeCoupling).unwrap();
        assert_eq!(s, "\"ScopeCoupling\"");
    }

    // --- BucklingOptionUnsupported tests (task 4149 — W_BucklingOptionUnsupported) ---
    // Pairs with `buckling_unsupported_option_diagnostics` in
    // `crates/reify-eval/src/compute_targets/buckling.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::BucklingOptionUnsupported` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` (mirrors the variant-agnostic
    /// `diagnostic_code_derives` shape but targeted at the new variant so a
    /// future enum reorganisation that drops it is caught here).
    ///
    /// The `Debug` repr is intentionally not asserted here: it is auto-derived
    /// cosmetic output with no consumer contract — a dropped variant would already
    /// be a compile error since the variant is referenced throughout.  The
    /// `d.code` round-trip and the serde wire-format test below are the
    /// behavioural contracts worth pinning.
    #[test]
    fn diagnostic_code_buckling_option_unsupported_with_code_round_trips() {
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::BucklingOptionUnsupported);
        assert_eq!(d.code, Some(DiagnosticCode::BucklingOptionUnsupported));
    }

    /// Under `feature = "serde"`, `DiagnosticCode::BucklingOptionUnsupported` serializes as
    /// `"BucklingOptionUnsupported"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_buckling_option_unsupported_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::BucklingOptionUnsupported).unwrap();
        assert_eq!(s, "\"BucklingOptionUnsupported\"");
    }

    // --- §7 shell-extract DiagnosticCode tests (task ε, #3837) ---
    // Six new PRD §7 extraction-failure codes.  Mirrors the
    // `diagnostic_code_stackup_variants_constructible` + `_serde_pascal_case`
    // pattern: construct via `Diagnostic::error("x").with_code(code)` (code and
    // severity round-trip) and assert PascalCase serde wire strings.

    /// All six new §7 shell-extract codes round-trip through
    /// `Diagnostic::error(...).with_code(...)` with `Severity::Error`.
    /// Mirrors the `diagnostic_code_stackup_variants_constructible` style.
    ///
    /// RED: the six variants do not exist → compile fail.
    /// GREEN after step-2 adds them to `DiagnosticCode`.
    #[test]
    fn diagnostic_code_shell_extract_variants_constructible() {
        use super::Severity;
        let codes = [
            DiagnosticCode::ShellNoVoxelGrid,
            DiagnosticCode::ShellMedialMaskOob,
            DiagnosticCode::ShellPruneFailed,
            DiagnosticCode::ShellMeshQuality,
            DiagnosticCode::ShellTooThick,
            DiagnosticCode::ShellNoMedial,
        ];
        for code in codes {
            let d = Diagnostic::error("x").with_code(code);
            assert_eq!(d.severity, Severity::Error, "severity mismatch for {code:?}");
            assert_eq!(d.code, Some(code), "code mismatch for {code:?}");
        }
    }

    /// Under `feature = "serde"`, each §7 shell-extract code serializes to its
    /// PascalCase wire string (from `rename_all = "PascalCase"`).
    /// Mirrors the `diagnostic_code_stackup_variants_serde_pascal_case` style.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_shell_extract_variants_serde_pascal_case() {
        let cases = [
            (DiagnosticCode::ShellNoVoxelGrid,   "\"ShellNoVoxelGrid\""),
            (DiagnosticCode::ShellMedialMaskOob, "\"ShellMedialMaskOob\""),
            (DiagnosticCode::ShellPruneFailed,   "\"ShellPruneFailed\""),
            (DiagnosticCode::ShellMeshQuality,   "\"ShellMeshQuality\""),
            (DiagnosticCode::ShellTooThick,      "\"ShellTooThick\""),
            (DiagnosticCode::ShellNoMedial,      "\"ShellNoMedial\""),
        ];
        for (code, expected) in cases {
            let s = serde_json::to_string(&code).unwrap();
            assert_eq!(s, expected, "serde mismatch for {code:?}");
        }
    }

    // --- MechanismNonDrivingJoint tests (task 4309 — E_MECHANISM_NONDRIVING_JOINT) ---
    // Pairs with the L1 eval guard in reify-stdlib snapshot.rs (bind arm) and
    // sweep.rs (dim/sweep/sweep_grid arms), and reserves the variant for the
    // L2 compile guard in reify-compiler (task γ). Per PRD D6: one code, two
    // emission sites. Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug
    // derives are already covered by `diagnostic_code_derives` above; only the
    // variant-specific round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::MechanismNonDrivingJoint` round-trips through
    /// `Diagnostic::error(...).with_code(...)`.
    /// Shape mirrors `diagnostic_code_unresolved_name_with_code_round_trips`
    /// (which targets a different variant); a future enum reorganisation that
    /// drops `MechanismNonDrivingJoint` is caught here.
    #[test]
    fn diagnostic_code_mechanism_nondriving_joint_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::error("x").with_code(DiagnosticCode::MechanismNonDrivingJoint);
        assert_eq!(d.code, Some(DiagnosticCode::MechanismNonDrivingJoint));
        assert_eq!(d.severity, Severity::Error);
    }

    /// Under `feature = "serde"`, `DiagnosticCode::MechanismNonDrivingJoint`
    /// serializes as `"MechanismNonDrivingJoint"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_mechanism_nondriving_joint_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::MechanismNonDrivingJoint).unwrap();
        assert_eq!(s, "\"MechanismNonDrivingJoint\"");
    }

    /// `DiagnosticInfo.has_location` round-trips through serde:
    /// (a) `has_location: false` serializes to JSON key `"has_location"` with value `false`
    /// — the field is never skipped, always present on the wire; (b) deserializing a JSON
    /// object that omits `has_location` yields `has_location == true` — pinning the
    /// backward-compat `#[serde(default = "default_has_location")]` contract so older
    /// payloads and un-updated consumers are treated as line-tied.
    ///
    /// RED until step-5 adds the field and `default_has_location` helper to `DiagnosticInfo`.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_info_has_location_serde_wire_and_default() {
        use super::DiagnosticInfo;

        // (a) Serialize: has_location: false must produce JSON key "has_location" = false.
        let info = DiagnosticInfo {
            file_path: "test.ri".to_owned(),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
            severity: "Error".to_owned(),
            message: "test".to_owned(),
            code: None,
            has_location: false,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(
            v["has_location"],
            serde_json::Value::Bool(false),
            "has_location: false must serialize to JSON false under key 'has_location'"
        );

        // (b) Deserialize: omitting has_location from JSON must yield has_location == true
        //     (backward-compat: older payloads without the field are treated as line-tied).
        let json = serde_json::json!({
            "file_path": "test.ri",
            "line": 1,
            "column": 1,
            "end_line": 1,
            "end_column": 1,
            "severity": "Error",
            "message": "test",
            "code": null
        });
        let deserialized: DiagnosticInfo = serde_json::from_value(json).unwrap();
        assert!(
            deserialized.has_location,
            "missing `has_location` in JSON must deserialize as true (backward-compat default)"
        );
    }

    // --- OpContractViolation tests (task 4323 — E_OP_CONTRACT) ---
    // Pairs with the undef-cause sink in `crates/reify-expr/src/lib.rs` (γ push
    // sites: FunctionCall arm after `eval_builtin`, `eval_binop` after the strict
    // undef-propagation check) and the `record_op_contract_failures` post-eval
    // helper in `crates/reify-eval/src/engine_eval.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::OpContractViolation` round-trips through
    /// `Diagnostic::error(...).with_code(...)` with `Severity::Error`.
    /// Shape mirrors `diagnostic_code_unresolved_name_with_code_round_trips`
    /// (which targets a different variant); a future enum reorganisation that
    /// drops `OpContractViolation` is caught here.
    ///
    /// RED: the variant does not exist → compile fail.
    /// GREEN after step-2 adds it to `DiagnosticCode`.
    #[test]
    fn diagnostic_code_op_contract_violation_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::error("x").with_code(DiagnosticCode::OpContractViolation);
        assert_eq!(d.code, Some(DiagnosticCode::OpContractViolation));
        assert_eq!(d.severity, Severity::Error);
    }

    /// Under `feature = "serde"`, `DiagnosticCode::OpContractViolation` serializes
    /// as `"OpContractViolation"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_op_contract_violation_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::OpContractViolation).unwrap();
        assert_eq!(s, "\"OpContractViolation\"");
    }

    // --- ReservedTypeName tests (task 4591 — W_RESERVED_TYPE_NAME) ---
    // Pairs with the lint pass in
    // `crates/reify-compiler/src/compile_builder/reserved_name_lint.rs`.
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip test is added here.

    /// Task 4591 (step-1): `DiagnosticCode::ReservedTypeName` must exist, be
    /// distinct from `DiagnosticCode::Shadowing`, and be attachable via
    /// `Diagnostic::warning(..).with_code(..)` with the code reading back.
    ///
    /// RED until step-2 adds the variant.
    #[test]
    fn reserved_type_name_code_exists_and_attaches() {
        // Exist + distinct from a neighbouring Warning code.
        assert_ne!(DiagnosticCode::ReservedTypeName, DiagnosticCode::Shadowing);

        // Attachable via the builder; code reads back correctly.
        let d = Diagnostic::warning("x").with_code(DiagnosticCode::ReservedTypeName);
        assert_eq!(d.code, Some(DiagnosticCode::ReservedTypeName));
    }

    // --- BareScalarType tests (task 4375 — E_BARE_SCALAR) ---
    // Pairs with the bare-Scalar guard in
    // `crates/reify-compiler/src/type_resolution.rs` (resolve_type_expr_with_aliases_kinded).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::BareScalarType` round-trips through
    /// `Diagnostic::error(...).with_code(...)` and the severity is `Severity::Error`.
    /// Shape mirrors `diagnostic_code_geometry_unbounded_with_code_round_trips`;
    /// a future enum reorganisation that drops `BareScalarType` is caught here.
    ///
    /// RED until step-2 adds the variant.
    #[test]
    fn diagnostic_code_bare_scalar_type_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::error(
            "bare `Scalar` is not a valid type: write `Scalar<Q>` or a named dimension like `Length`",
        )
        .with_code(DiagnosticCode::BareScalarType);
        assert_eq!(d.code, Some(DiagnosticCode::BareScalarType));
        assert_eq!(d.severity, Severity::Error);
    }

    /// Under `feature = "serde"`, `DiagnosticCode::BareScalarType` serializes as
    /// `"BareScalarType"` (PascalCase, from `rename_all = "PascalCase"`).
    ///
    /// RED until step-2 adds the variant.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_bare_scalar_type_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::BareScalarType).unwrap();
        assert_eq!(s, "\"BareScalarType\"");
    }

    // --- TypeArgArity / TypeArgBound tests (task 4603 γ — E_TYPE_ARG_ARITY / E_TYPE_ARG_BOUND) ---
    // Pairs with `check_applied_type_arg_bounds` in
    // `crates/reify-compiler/src/type_resolution.rs` (called from
    // `phase_pending_bound_checks` in entities_phase.rs).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::TypeArgArity` round-trips through
    /// `Diagnostic::error(...).with_code(...)`.
    ///
    /// RED until step-4 adds the variant.
    #[test]
    fn diagnostic_code_type_arg_arity_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::error("wrong number of type arguments").with_code(DiagnosticCode::TypeArgArity);
        assert_eq!(d.code, Some(DiagnosticCode::TypeArgArity));
        assert_eq!(d.severity, Severity::Error);
    }

    /// `DiagnosticCode::TypeArgBound` round-trips through
    /// `Diagnostic::error(...).with_code(...)`.
    ///
    /// RED until step-4 adds the variant.
    #[test]
    fn diagnostic_code_type_arg_bound_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::error("type argument does not satisfy bound").with_code(DiagnosticCode::TypeArgBound);
        assert_eq!(d.code, Some(DiagnosticCode::TypeArgBound));
        assert_eq!(d.severity, Severity::Error);
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TypeArgArity` serializes as
    /// `"TypeArgArity"` (PascalCase, from `rename_all = "PascalCase"`).
    ///
    /// RED until step-4 adds the variant.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_type_arg_arity_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TypeArgArity).unwrap();
        assert_eq!(s, "\"TypeArgArity\"");
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TypeArgBound` serializes as
    /// `"TypeArgBound"` (PascalCase, from `rename_all = "PascalCase"`).
    ///
    /// RED until step-4 adds the variant.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_type_arg_bound_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TypeArgBound).unwrap();
        assert_eq!(s, "\"TypeArgBound\"");
    }

    // --- TopologyCorrespondenceDropped tests (task 4545 — W_TOPOLOGY_CORRESPONDENCE_DROPPED) ---
    // Pairs with diagnose_topology_correspondence_drops in
    // `crates/reify-eval/src/engine_build.rs` (wired in execute_realization_ops).
    // Variant-agnostic Copy/Clone/PartialEq/Eq/Hash/Debug derives are already
    // covered by `diagnostic_code_derives` above; only the variant-specific
    // round-trip and serde wire-format tests are added here.

    /// `DiagnosticCode::TopologyCorrespondenceDropped` round-trips through
    /// `Diagnostic::warning(...).with_code(...)` carrying both the expected
    /// `Severity::Warning` and `Some(DiagnosticCode::TopologyCorrespondenceDropped)`.
    /// Pins the warning-severity contract and variant existence for the
    /// topology-correspondence-drop diagnostic (PRD-prose mnemonic
    /// W_TOPOLOGY_CORRESPONDENCE_DROPPED).
    #[test]
    fn diagnostic_code_topology_correspondence_dropped_with_code_round_trips() {
        use super::Severity;
        let d = Diagnostic::warning("x")
            .with_code(DiagnosticCode::TopologyCorrespondenceDropped);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TopologyCorrespondenceDropped)
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TopologyCorrespondenceDropped`
    /// serializes as `"TopologyCorrespondenceDropped"` (PascalCase, from
    /// `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_topology_correspondence_dropped_serde_pascal_case() {
        let s =
            serde_json::to_string(&DiagnosticCode::TopologyCorrespondenceDropped).unwrap();
        assert_eq!(s, "\"TopologyCorrespondenceDropped\"");
    }

    /// Task 3584 θ (step-1/step-2): `ProgressiveInvariantViolated` (W_PROGRESSIVE_INVARIANT_VIOLATED)
    /// must exist, be distinct from an existing W_* code (`ReservedTypeName`), and be attachable
    /// via `Diagnostic::warning(..).with_code(..)` — reads back `code == Some(...)` and
    /// `severity == Severity::Warning`.
    ///
    /// RED until step-2 adds the `ProgressiveInvariantViolated` variant to `DiagnosticCode`.
    #[test]
    fn progressive_invariant_violated_code_exists_and_attaches() {
        use super::Severity;

        // Exists + distinct from another W_* code.
        assert_ne!(
            DiagnosticCode::ProgressiveInvariantViolated,
            DiagnosticCode::ReservedTypeName,
        );

        // Attachable via the warning builder; code and severity read back.
        let diag = Diagnostic::warning(
            "node 'value cell Bracket.width' wrote Freshness::Intermediate without the PROGRESSIVE trait",
        )
        .with_code(DiagnosticCode::ProgressiveInvariantViolated);
        assert_eq!(diag.code, Some(DiagnosticCode::ProgressiveInvariantViolated));
        assert_eq!(diag.severity, Severity::Warning);
    }

    /// Task 2992 step-1 (RED): `hex_wedge_mesh_diagnostic` with `Promoted{hex_count:4,
    /// wedge_count:2}` and `require_hex_wedge=false` must emit `Severity::Info`,
    /// `code==Some(DiagnosticCode::HexWedgePromoted)`, and
    /// `message=="Body B1 meshed as 4 hex / 2 wedge"`.
    ///
    /// Additionally, the same outcome with `require_hex_wedge=true` must STILL be
    /// `Severity::Info` (success is never upgraded regardless of the require flag).
    ///
    /// RED until step-2 adds `DiagnosticCode::HexWedgePromoted`, `HexWedgeMeshOutcome`,
    /// and `hex_wedge_mesh_diagnostic`.
    #[test]
    fn hex_wedge_promoted_emits_info_with_code_and_count_message() {
        use super::{HexWedgeMeshOutcome, Severity, hex_wedge_mesh_diagnostic};

        let outcome = HexWedgeMeshOutcome::Promoted { hex_count: 4, wedge_count: 2 };

        // require_hex_wedge=false: Info + correct code + exact message
        let d = hex_wedge_mesh_diagnostic(&outcome, false, "B1");
        assert_eq!(d.severity, Severity::Info);
        assert_eq!(d.code, Some(DiagnosticCode::HexWedgePromoted));
        assert_eq!(d.message, "Body B1 meshed as 4 hex / 2 wedge");

        // require_hex_wedge=true: success is NEVER upgraded — still Info
        let d2 = hex_wedge_mesh_diagnostic(&outcome, true, "B1");
        assert_eq!(d2.severity, Severity::Info);
        assert_eq!(d2.code, Some(DiagnosticCode::HexWedgePromoted));

        // Degenerate case: zero hex and zero wedge counts — doc explicitly
        // allows this; confirm message formats sensibly and severity stays Info.
        let zero_outcome = HexWedgeMeshOutcome::Promoted { hex_count: 0, wedge_count: 0 };
        let d3 = hex_wedge_mesh_diagnostic(&zero_outcome, false, "B2");
        assert_eq!(d3.severity, Severity::Info);
        assert_eq!(d3.code, Some(DiagnosticCode::HexWedgePromoted));
        assert_eq!(d3.message, "Body B2 meshed as 0 hex / 0 wedge");
    }

    /// Task 2992 step-3 (RED→GREEN): the three genuine fall-back causes default
    /// to `Severity::Info` with distinct diagnostic codes, and each message
    /// contains the body label.
    ///
    /// Verifies that with `require_hex_wedge=false`:
    /// - `PhaseAFinishingOps` → `Info` + `HexWedgePhaseAFinishingOps` + message ∋ "B1"
    /// - `InvalidSweepGeometry` → `Info` + `HexWedgeInvalidSweepGeometry` + message ∋ "B1"
    /// - `Mesh2dFailure` → `Info` + `HexWedge2dMeshFailure` + message ∋ "B1"
    ///
    /// RED until step-4 adds these three variants to `HexWedgeMeshOutcome` and their
    /// arms to `hex_wedge_mesh_diagnostic`.
    #[test]
    fn hex_wedge_fallback_causes_default_to_info_with_distinct_codes() {
        use super::{HexWedgeMeshOutcome, Severity, hex_wedge_mesh_diagnostic};

        // The durable contract is the distinct DiagnosticCode per arm; the
        // body-label check ensures the message references the body being meshed.
        // Prose-substring pinning is intentionally omitted — the code assertions
        // already distinguish the arms, and wording changes should not break tests.
        let cases = [
            (HexWedgeMeshOutcome::PhaseAFinishingOps, DiagnosticCode::HexWedgePhaseAFinishingOps),
            (
                HexWedgeMeshOutcome::InvalidSweepGeometry,
                DiagnosticCode::HexWedgeInvalidSweepGeometry,
            ),
            (HexWedgeMeshOutcome::Mesh2dFailure, DiagnosticCode::HexWedge2dMeshFailure),
        ];

        for (outcome, expected_code) in &cases {
            let d = hex_wedge_mesh_diagnostic(outcome, false, "B1");
            assert_eq!(d.severity, Severity::Info, "expected Info for {expected_code:?}");
            assert_eq!(d.code, Some(*expected_code), "wrong code for {expected_code:?}");
            assert!(
                d.message.contains("B1"),
                "message should mention body label 'B1', got: {:?}",
                d.message
            );
        }

        // All three codes must be distinct.
        assert_ne!(
            DiagnosticCode::HexWedgePhaseAFinishingOps,
            DiagnosticCode::HexWedgeInvalidSweepGeometry
        );
        assert_ne!(
            DiagnosticCode::HexWedgeInvalidSweepGeometry,
            DiagnosticCode::HexWedge2dMeshFailure
        );
        assert_ne!(
            DiagnosticCode::HexWedgePhaseAFinishingOps,
            DiagnosticCode::HexWedge2dMeshFailure
        );
    }

    /// Task 2992 step-5 (RED→GREEN): `require_hex_wedge=true` upgrades all three
    /// genuine fall-back causes from `Info` to `Error`.
    ///
    /// Verifies that the `DiagnosticCode` is **unchanged** (same variant as the
    /// Info path) and that the message is also unchanged, so downstream tooling
    /// can match the cause independent of severity.
    ///
    /// RED until step-6 implements the severity-selection rule in
    /// `hex_wedge_mesh_diagnostic`.
    #[test]
    fn hex_wedge_require_hex_wedge_upgrades_fallbacks_to_error() {
        use super::{HexWedgeMeshOutcome, Severity, hex_wedge_mesh_diagnostic};

        let cases = [
            (
                HexWedgeMeshOutcome::PhaseAFinishingOps,
                DiagnosticCode::HexWedgePhaseAFinishingOps,
            ),
            (
                HexWedgeMeshOutcome::InvalidSweepGeometry,
                DiagnosticCode::HexWedgeInvalidSweepGeometry,
            ),
            (
                HexWedgeMeshOutcome::Mesh2dFailure,
                DiagnosticCode::HexWedge2dMeshFailure,
            ),
        ];

        for (outcome, expected_code) in &cases {
            // require_hex_wedge=false baseline
            let info_d = hex_wedge_mesh_diagnostic(outcome, false, "B1");
            // require_hex_wedge=true must upgrade to Error
            let err_d = hex_wedge_mesh_diagnostic(outcome, true, "B1");

            assert_eq!(
                err_d.severity,
                Severity::Error,
                "expected Error with require_hex_wedge=true for {expected_code:?}"
            );
            // Code is preserved across the severity upgrade.
            assert_eq!(
                err_d.code,
                Some(*expected_code),
                "code must be unchanged for {expected_code:?}"
            );
            // Message is unchanged.
            assert_eq!(
                err_d.message, info_d.message,
                "message must be unchanged across severity upgrade for {expected_code:?}"
            );
        }
    }

    /// Task 2992 step-7 (RED→GREEN): `ForceTet` emits `Severity::Info` with the
    /// `HexWedgeForceTet` code.
    ///
    /// The PRD "debug / debug (no upgrade)" rule means `ForceTet` is
    /// upgrade-exempt: `require_hex_wedge=true` may not legally co-occur with a
    /// `ForceTet` outcome (the stdlib enforces `!(force_tet && require_hex_wedge)`
    /// and `hex_wedge_mesh_diagnostic` has a `debug_assert!` to catch violations
    /// at the call boundary), so the upgrade path is guarded rather than tested.
    ///
    /// Also verifies the `HexWedgeForceTet` code is distinct from the
    /// fall-back codes and from `HexWedgePromoted`.
    ///
    /// RED until step-8 adds `HexWedgeForceTet`, the `ForceTet` variant, and
    /// its arm in `hex_wedge_mesh_diagnostic`.
    #[test]
    fn hex_wedge_force_tet_is_info_and_upgrade_exempt() {
        use super::{HexWedgeMeshOutcome, Severity, hex_wedge_mesh_diagnostic};

        let outcome = HexWedgeMeshOutcome::ForceTet;

        // The only valid call: force_tet and require_hex_wedge are mutually exclusive
        // by stdlib constraint, so require_hex_wedge must always be false here.
        // hex_wedge_mesh_diagnostic has a debug_assert! that catches violations.
        let d = hex_wedge_mesh_diagnostic(&outcome, false, "B1");
        assert_eq!(d.severity, Severity::Info);
        assert_eq!(d.code, Some(DiagnosticCode::HexWedgeForceTet));

        // Distinct from other HexWedge* codes.
        assert_ne!(DiagnosticCode::HexWedgeForceTet, DiagnosticCode::HexWedgePromoted);
        assert_ne!(
            DiagnosticCode::HexWedgeForceTet,
            DiagnosticCode::HexWedgePhaseAFinishingOps
        );
        assert_ne!(
            DiagnosticCode::HexWedgeForceTet,
            DiagnosticCode::HexWedgeInvalidSweepGeometry
        );
        assert_ne!(DiagnosticCode::HexWedgeForceTet, DiagnosticCode::HexWedge2dMeshFailure);
    }

    /// Task 2992 amendment: pin the `debug_assert!` invariant guard in
    /// `hex_wedge_mesh_diagnostic` that prevents a `ForceTet` outcome from being
    /// paired with `require_hex_wedge=true`.
    ///
    /// Runs only in debug builds (`cfg(debug_assertions)`), where `debug_assert!`
    /// fires.  This ensures a future refactor that weakens or removes the guard is
    /// caught by the test suite rather than silently regressing.
    ///
    /// The valid `require_hex_wedge=false` path (the only legal call when
    /// `force_tet=true`) is covered by `hex_wedge_force_tet_is_info_and_upgrade_exempt`.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "force_tet and require_hex_wedge are mutually exclusive")]
    fn hex_wedge_force_tet_with_require_hex_wedge_panics_in_debug() {
        use super::{HexWedgeMeshOutcome, hex_wedge_mesh_diagnostic};
        // ForceTet + require_hex_wedge=true violates the stdlib constraint
        // !(force_tet && require_hex_wedge); the debug_assert! must catch this.
        hex_wedge_mesh_diagnostic(&HexWedgeMeshOutcome::ForceTet, true, "B1");
    }

    /// Task 2992 step-9 (RED→GREEN): the `PhaseAFinishingOps` diagnostic message
    /// must contain the substring `"Phase B"`, pinning the PRD's requirement that
    /// the post-sweep-modifications fall-back notes that Phase B (PRD task #14,
    /// axial-finishing operations) will eventually handle this case.
    ///
    /// Only the `Info` path (`require_hex_wedge=false`) is asserted here, because
    /// `hex_wedge_require_hex_wedge_upgrades_fallbacks_to_error` already verifies
    /// that the `Error` path uses an identical message string.  Checking both paths
    /// against `"Phase B"` would pin the same prose twice, making innocuous wording
    /// changes break two tests instead of one without adding coverage.
    ///
    /// RED until step-10 finalizes the `PhaseAFinishingOps` message wording.
    #[test]
    fn hex_wedge_phase_a_message_mentions_phase_b() {
        use super::{HexWedgeMeshOutcome, hex_wedge_mesh_diagnostic};

        let outcome = HexWedgeMeshOutcome::PhaseAFinishingOps;

        // Verify on the Info path only — message equality across severity levels is
        // already covered by hex_wedge_require_hex_wedge_upgrades_fallbacks_to_error.
        let info_d = hex_wedge_mesh_diagnostic(&outcome, false, "B1");
        assert!(
            info_d.message.contains("Phase B"),
            "PhaseAFinishingOps message must mention 'Phase B', got: {:?}",
            info_d.message
        );
    }

    // --- TypeUndetermined tests (task #4703 δ) ---
    // Pairs with the FunctionCall arg-pushdown producer in
    // `crates/reify-compiler/src/expr.rs` (argument-position empty-collection-literal
    // whose parameter element is an unbound function type-parameter).

    /// `DiagnosticCode::TypeUndetermined` round-trips through
    /// `Diagnostic::error(...).with_code(...)` (mirrors the GeometryUnbounded shape
    /// so a future enum reorganisation that drops it is caught here).
    #[test]
    fn diagnostic_code_type_undetermined_with_code_round_trips() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::TypeUndetermined);
        assert_eq!(d.code, Some(DiagnosticCode::TypeUndetermined));
        assert_eq!(
            format!("{:?}", DiagnosticCode::TypeUndetermined),
            "TypeUndetermined"
        );
    }

    /// Under `feature = "serde"`, `DiagnosticCode::TypeUndetermined` serializes as
    /// `"TypeUndetermined"` (PascalCase, from `rename_all = "PascalCase"`).
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_type_undetermined_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TypeUndetermined).unwrap();
        assert_eq!(s, "\"TypeUndetermined\"");
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
    /// Whether this diagnostic carries a real, line-tied source span.
    ///
    /// `true` means the `line`/`column`/`end_line`/`end_column` fields reflect an
    /// actual span from the compiled source (non-empty `Diagnostic::labels`).
    /// `false` means the positions are synthetic (hardcoded 1/1/1/1) and do NOT
    /// point at a meaningful source location — e.g. module-level hot-reload staleness
    /// errors where no span is available.
    ///
    /// Consumers (β span-less render, γ span-less refusal) use this flag to avoid
    /// navigating the editor to a fake line 1 for span-less diagnostics.
    ///
    /// **Wire default:** a JSON payload that omits `has_location` deserializes as
    /// `true` (line-tied) to preserve backward compatibility with older serializers
    /// and un-updated consumers.
    #[cfg_attr(feature = "serde", serde(default = "default_has_location"))]
    pub has_location: bool,
}

/// Serde default for [`DiagnosticInfo::has_location`]: `true` (line-tied).
///
/// Returning `true` makes a JSON payload that omits `has_location` deserialize as
/// line-tied, preserving backward-compat for older serializers and un-updated consumers.
#[cfg(feature = "serde")]
fn default_has_location() -> bool {
    true
}
