//! Shared test helpers.
//!
//! Most of this module is the `eval-helpers`-gated pipeline for parsing,
//! compiling, and evaluating Reify source in tests. Alongside it sit un-gated
//! helpers that need no engine: [`collect_value_ref_members`], which inspects
//! an already-compiled expression, and [`missing_paths_under`], a filesystem
//! path-existence filter shared by the test suites' skip-list guards.

use std::path::Path;

use reify_compiler::TopologyTemplate;
use reify_core::{Diagnostic, DiagnosticLabel, ModulePath, Severity};
use reify_ir::{CompiledExpr, CompiledExprKind};

#[cfg(feature = "eval-helpers")]
use crate::mocks::{MockConstraintChecker, MockGeometryKernel};

/// Collect all `ValueRef` member names reachable from `expr`.
///
/// This is the canonical, walk-backed replacement for the per-file
/// `collect_value_ref_members` copies that existed in the compiler test suite.
/// Unlike those hand-rolled copies (which matched only `ValueRef`, `BinOp`, and
/// `UnOp`, dropping refs under any other variant via `_ => vec![]`), this
/// implementation is backed by [`CompiledExpr::walk`], which performs an
/// exhaustive pre-order traversal of **every** [`CompiledExprKind`] variant.
/// This means `ValueRef` nodes nested under `FunctionCall`, `Conditional`,
/// `OptionSome`, `ListLiteral`, etc. are now correctly collected (item-#3 fix).
/// When a new `CompiledExprKind` variant is added, `walk`'s exhaustive match
/// forces an update there, and this helper benefits automatically.
///
/// Returns owned `String`s (not `&str`s) because `walk`'s closure parameter
/// has a higher-ranked lifetime — a borrow of `cell_id.member` cannot escape
/// into a `Vec<&str>` tied to `expr`'s lifetime.
pub fn collect_value_ref_members(expr: &CompiledExpr) -> Vec<String> {
    let mut members = Vec::new();
    expr.walk(&mut |e| {
        if let CompiledExprKind::ValueRef(cell_id) = &e.kind {
            members.push(cell_id.member.clone());
        }
    });
    members
}

/// Return the subset of `rel_paths` that have no filesystem entry at
/// `dir.join(rel)`.
///
/// This is the single source of truth for the SKIP_SET dead-key check — the
/// guard that catches a skip-list entry naming a file that has since been
/// renamed or deleted, which would otherwise silently disable coverage
/// forever. It replaced the per-file copies of this `Path::exists` filter that
/// each such guard used to open-code. This doc is the only place their shared
/// contract is stated: a call site carries a pointer back here, not a copy.
///
/// # Contracts callers may rely on
///
/// - **The full offending set is returned.** This never short-circuits on the
///   first miss, so a caller can report every stale key in one panic instead
///   of forcing an operator to fix them one run at a time.
/// - **Input order is preserved** (`filter` is order-preserving), so callers
///   need not sort to get a stable, reviewable failure message.
///
/// # Arity is the caller's problem
///
/// Skip lists carry per-file metadata of differing shape, so this takes a
/// plain iterator of relative paths and callers project their own tuple away
/// at the call boundary — `SKIP_SET.iter().map(|(rel, _)| *rel)`. That is what
/// lets skip lists of differing arity share one implementation while staying
/// private to their own crate: no cross-crate coupling of the skip lists is
/// created or implied.
///
/// # Filesystem semantics
///
/// Existence is [`Path::exists`], which follows symlinks and does not
/// distinguish a file from a directory. A broken symlink therefore reports as
/// *missing* — pinned by
/// `test_missing_paths_under_reports_dangling_symlink_as_missing` below.
///
/// Any other condition under which `Path::exists` answers `false` — an
/// unreadable parent directory, say — likewise reports as *missing*. That is a
/// consequence of `Path::exists`, not a separately pinned behaviour: no test
/// below exercises it.
pub fn missing_paths_under<'a>(
    dir: &Path,
    rel_paths: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    rel_paths
        .into_iter()
        .filter(|rel| !dir.join(rel).exists())
        .collect()
}

/// Create a new `Engine` backed by a fresh `MockConstraintChecker` and no
/// geometry kernel. Suitable for tests that only need to evaluate logic
/// expressions and constraints without real geometry.
#[cfg(feature = "eval-helpers")]
pub fn make_engine() -> reify_eval::Engine {
    let checker = MockConstraintChecker::new();
    reify_eval::Engine::new(Box::new(checker), None)
}

/// Create a new `Engine` backed by the real `SimpleConstraintChecker` and no
/// geometry kernel. Suitable for integration tests that need the real
/// constraint semantics (Satisfied/Violated/Indeterminate) rather than the
/// mock's tracking-only stub.
#[cfg(feature = "eval-helpers")]
pub fn make_simple_engine() -> reify_eval::Engine {
    reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None)
}

/// Parse, compile (asserting no errors), and evaluate `source` using a
/// `MockConstraintChecker` engine. Returns the `EvalResult`.
///
/// Convenience pipeline for eval tests that need value evaluation without
/// real constraint semantics.
///
/// # Panics
/// Panics on parse errors, compile errors, or eval-phase diagnostics.
#[cfg(feature = "eval-helpers")]
pub fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval-phase errors: {:?}",
        eval_errors
    );
    result
}

/// Parse, compile (asserting no errors), and check `source` using a
/// `SimpleConstraintChecker` engine. Returns the `CheckResult`.
///
/// Convenience pipeline for tests that need real constraint satisfaction
/// semantics (Satisfied/Violated/Indeterminate).
///
/// # Panics
/// Panics on parse errors or compile errors.
#[cfg(feature = "eval-helpers")]
pub fn check_source(source: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile(source);
    let mut engine = make_simple_engine();
    engine.check(&compiled)
}

/// Parse, compile with stdlib (asserting no errors), and check `source`
/// using a `SimpleConstraintChecker` engine. Returns the `CheckResult`.
///
/// Like [`check_source`] but uses `parse_and_compile_with_stdlib` so that
/// stdlib types and traits are available during compilation.
///
/// # Panics
/// Panics on parse errors or compile errors.
#[cfg(feature = "eval-helpers")]
pub fn check_source_with_stdlib(source: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    engine.check(&compiled)
}

/// Parse `source` with the given `module_name` module path, asserting no parse errors.
///
/// # Panics
/// Panics if there are any parse errors.
fn parse_or_panic_named(source: &str, module_name: &str) -> reify_ast::ParsedModule {
    let parsed = reify_syntax::parse(source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    parsed
}

/// Parse `source` with the canonical `"test"` module path, asserting no parse errors.
///
/// # Panics
/// Panics if there are any parse errors.
fn parse_or_panic(source: &str) -> reify_ast::ParsedModule {
    parse_or_panic_named(source, "test")
}

/// Parse `source` with the canonical `"test"` module path AND pre-seed the
/// parser with stdlib enum names via [`reify_compiler::parse_with_stdlib`].
/// Used by the `_with_stdlib` helpers so prelude/stdlib enum references like
/// `CorrosionClass.C5` lower to `EnumAccess` rather than `MemberAccess`.
///
/// # Panics
/// Panics if there are any parse errors.
fn parse_with_stdlib_or_panic(source: &str) -> reify_ast::ParsedModule {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    parsed
}

/// Parse and compile `source` without asserting absence of compile errors.
/// Returns the compiled module with whatever diagnostics were produced.
///
/// Use this for tests that expect compilation errors/warnings. For tests
/// that expect clean compilation, use [`parse_and_compile`] instead.
///
/// # Panics
/// Panics if there are any parse errors (but NOT compile errors).
pub fn compile_source(source: &str) -> reify_compiler::CompiledModule {
    let parsed = parse_or_panic(source);
    reify_compiler::compile(&parsed)
}

/// Like [`compile_source`] but uses a custom `module_name` for the module path
/// instead of the default `"test"`.
///
/// Useful when diagnostic or debug output should show a descriptive module name
/// rather than the generic `"test"`.
///
/// # Panics
/// Panics if there are any parse errors (but NOT compile errors).
pub fn compile_source_named(source: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = parse_or_panic_named(source, module_name);
    reify_compiler::compile(&parsed)
}

/// Parse and compile `source` with stdlib, without asserting absence of compile errors.
///
/// Like [`compile_source`] but uses `reify_compiler::compile_with_stdlib` so that
/// stdlib types and traits are available during compilation, and routes the
/// parse step through `reify_compiler::parse_with_stdlib` so prelude-stdlib
/// enum names participate in `EnumAccess` disambiguation.
///
/// # Panics
/// Panics if there are any parse errors (but NOT compile errors).
pub fn compile_source_with_stdlib(source: &str) -> reify_compiler::CompiledModule {
    let parsed = parse_with_stdlib_or_panic(source);
    reify_compiler::compile_with_stdlib(&parsed)
}

/// Build the function table an `EvalContext` needs in order to evaluate a module
/// compiled by [`compile_source_with_stdlib`] the way PRODUCTION does.
///
/// # Why this exists
///
/// [`compile_source_with_stdlib`] returns a [`reify_compiler::CompiledModule`]
/// whose `.functions` is **user-source-only**. The compile pipeline pushes only
/// the module's own AST fns into `ctx.functions`
/// (`compile_builder/functions_phase.rs:79-92`); prelude fns arrive through a
/// *different* parameter and are flat-mapped into `ctx.resolution_functions`
/// only (`functions_phase.rs:101-105`, re-run at `traits_phase.rs:194-200`), and
/// `ctx.rs:201` (`functions: self.functions`) then drops that prelude table when
/// the `CompiledModule` is built.
///
/// So `EvalContext::new(&values, &module.functions)` evaluates against a table
/// in which **no stdlib `.ri` body is registered at all**. Every stdlib call
/// misses [`reify_expr::find_matching_compiled_function`] and falls to the
/// lookup-miss arm (`reify-expr/src/lib.rs:1625`), which returns `Value::Undef`.
/// A test written against that table can never observe a stdlib placeholder
/// body, so it cannot distinguish "the intercept fired" from "the intercept is
/// missing and the call simply evaluated to nothing".
///
/// Passing the slice this function returns instead restores the missing
/// competitor: the stdlib `.ri` bodies are registered and genuinely compete with
/// the reify-expr intercepts.
///
/// # Why the append is UNFILTERED
///
/// This mirrors [`reify_eval::merge_functions`] (`reify-eval/src/lib.rs:1425-1432`,
/// `pub(crate)` so it cannot be called directly) — the table PRODUCTION
/// evaluation actually uses — and deliberately NOT
/// [`reify_compiler::merge_prelude_functions`] (`reify-compiler/src/lib.rs:332`).
///
/// The two differ on collisions. `merge_prelude_functions` FILTERS out any
/// prelude entry whose `(name, arity, param_types)` triple matches a user fn,
/// because it builds the COMPILE-TIME overload-resolution table where a
/// duplicate triple is an ambiguous-overload error. reify-eval omits that filter
/// (its own doc, `lib.rs:1412-1419`) because dispatch is a first-match-wins
/// linear scan, so a shadowed prelude entry is permanently unreachable anyway.
///
/// The two are therefore dispatch-EQUIVALENT; this is a fidelity choice, not a
/// behaviour fix. But a harness whose entire purpose is "evaluate the way
/// production does" should not quietly diverge from production in how it builds
/// its table, so the runtime shape is what gets copied. User functions stay
/// FIRST so they still shadow prelude functions, exactly as at runtime.
///
/// # Example
///
/// ```ignore
/// let module = compile_source_with_stdlib("structure S { let v = through(5mm) }");
/// let expr = get_let_expr(&module, "v");
/// let values = ValueMap::new();
/// let functions = prelude_backed_functions(&module);
/// let ctx = EvalContext::new(&values, &functions);
/// ```
pub fn prelude_backed_functions(
    module: &reify_compiler::CompiledModule,
) -> Vec<reify_ir::CompiledFunction> {
    // Flatten the prelude exactly as `reify_eval::Engine::with_prelude_and_kernels`
    // does (reify-eval/src/engine_admin.rs:255-258) when populating its
    // `prelude_functions` field.
    let prelude = reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .flat_map(|m| m.functions.iter().cloned());

    // ...then merge with the same two lines as `reify_eval::merge_functions`
    // (reify-eval/src/lib.rs:1429-1431): user fns first, prelude appended
    // unconditionally.
    let mut merged = module.functions.clone();
    merged.extend(prelude);
    merged
}

/// Convert parse-layer [`reify_ast::ParseError`]s into `Severity::Error`
/// [`Diagnostic`]s so they can be surfaced through a `CompiledModule`'s
/// `diagnostics` list. Each parse error's span is attached as a label.
fn parse_errors_as_diagnostics(parsed: &reify_ast::ParsedModule) -> Vec<Diagnostic> {
    parsed
        .errors
        .iter()
        .map(|e| {
            Diagnostic::error(e.message.clone())
                .with_label(DiagnosticLabel::new(e.span, e.message.clone()))
        })
        .collect()
}

/// Parse and compile `source` WITHOUT asserting absence of parse errors,
/// forwarding any parse-layer diagnostics into the returned module's
/// `diagnostics` list (prepended ahead of compile-layer diagnostics).
///
/// Use this for tests that exercise rejection now emitted at the *parse*
/// layer — e.g. out-of-range numeric literals, which task #4681 moved from a
/// compile-layer `!is_finite()` guard to a parse-layer range check. Unlike
/// [`compile_source`] (which calls `parse_or_panic` and aborts on any parse
/// error), this helper lets the test inspect the combined diagnostics via
/// `errors_only(&module)` exactly as it would for a compile-layer rejection.
///
/// Compilation still runs on whatever (possibly partial) AST the parser
/// produced, so downstream invariants like "the offending unit is NOT
/// registered" remain observable.
pub fn compile_source_allow_parse_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    let mut diagnostics = parse_errors_as_diagnostics(&parsed);
    let mut compiled = reify_compiler::compile(&parsed);
    diagnostics.append(&mut compiled.diagnostics);
    compiled.diagnostics = diagnostics;
    compiled
}

/// Like [`compile_source_allow_parse_errors`] but parses with the stdlib
/// prelude enum names pre-seeded and compiles with the full stdlib context
/// (mirrors [`compile_source_with_stdlib`]).
pub fn compile_source_with_stdlib_allow_parse_errors(
    source: &str,
) -> reify_compiler::CompiledModule {
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    let mut diagnostics = parse_errors_as_diagnostics(&parsed);
    let mut compiled = reify_compiler::compile_with_stdlib(&parsed);
    diagnostics.append(&mut compiled.diagnostics);
    compiled.diagnostics = diagnostics;
    compiled
}

/// Parse and compile `source`, then extract the first template.
/// Returns the template and the full list of diagnostics.
///
/// # Panics
/// Panics if there are parse errors or if the compiled module has no templates.
pub fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let compiled = compile_source(source);
    let diagnostics = compiled.diagnostics;
    let template = compiled
        .templates
        .into_iter()
        .next()
        .expect("compile_first_template: no templates in compiled module");
    (template, diagnostics)
}

/// Parse and compile `source`, then extract the template with the given `name`.
/// Returns the template and the full list of diagnostics.
///
/// # Panics
/// Panics if there are parse errors or if no template with `name` is found.
pub fn compile_template(source: &str, name: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let compiled = compile_source(source);
    let diagnostics = compiled.diagnostics;
    let template = compiled
        .templates
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("compile_template: template {:?} not found", name));
    (template, diagnostics)
}

/// Filter a diagnostic slice to only `Severity::Error` entries.
///
/// This is the primitive; [`errors_only`] is the convenience wrapper
/// that takes a `&CompiledModule`.
pub fn collect_errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Return only the `Severity::Error` entries from a diagnostic slice.
///
/// Short-named convenience wrapper for use in eval-integration tests where
/// the filter-collect pattern `.filter(|d| d.severity == Severity::Error)
/// .collect::<Vec<_>>()` otherwise repeats at every assertion site.
///
/// Semantically equivalent to [`collect_errors`]; prefer this name when the
/// returned value will be bound to a local called `error_diags` so the call
/// site reads as `let error_diags = error_diags(&diags);`.
pub fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Return only the `Severity::Error` diagnostics from a compiled module.
///
/// Convenience wrapper around [`collect_errors`].
pub fn errors_only(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    collect_errors(&module.diagnostics)
}

/// Return only the `Severity::Warning` diagnostics from a compiled module.
pub fn warnings_only(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

/// Assert that an [`reify_eval::EvalResult`] contains no Error-severity
/// diagnostics. Panics with diagnostic details if any errors are found.
///
/// # Panics
/// Panics if `result.diagnostics` contains any [`reify_types::Severity::Error`]
/// entry. The panic message lists all error diagnostics for easy debugging.
#[cfg(feature = "eval-helpers")]
#[track_caller]
pub fn assert_no_eval_errors(result: &reify_eval::EvalResult) {
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "eval errors: {:?}", errors);
}

/// Assert that a [`reify_eval::CheckResult`] contains no Error-severity diagnostics.
///
/// Use this immediately after `engine.check(&compiled)` — before inspecting
/// `constraint_results` or `values` — so that eval-phase errors produce a precise
/// failure message rather than an opaque `unwrap()`/index-out-of-bounds panic.
///
/// # Panics
/// Panics if `result.diagnostics` contains any [`reify_types::Severity::Error`]
/// entry. The panic message lists all error diagnostics for easy debugging.
#[cfg(feature = "eval-helpers")]
#[track_caller]
pub fn assert_no_check_errors(result: &reify_eval::CheckResult) {
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "check errors: {:?}", errors);
}

/// Assert that an [`reify_eval::EvalResult`] contains no diagnostics of any severity.
///
/// This is stricter than [`assert_no_eval_errors`]: it also fails on Warning, Info,
/// and any other severity. Use this when the expected outcome is completely clean — no
/// errors, no warnings, no informational messages. This is appropriate for tests
/// verifying simple, well-formed inputs where any diagnostic indicates a regression.
///
/// # Panics
/// Panics if `result.diagnostics` is non-empty. The panic message lists all diagnostics.
#[cfg(feature = "eval-helpers")]
#[track_caller]
pub fn assert_eval_clean(result: &reify_eval::EvalResult) {
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        result.diagnostics
    );
}

/// Parse `source`, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module ready for eval.
///
/// # Panics
/// Panics if there are any parse errors or error-severity compile diagnostics.
pub fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

/// Parse `source`, assert no parse errors, compile with stdlib, assert no compile errors.
/// Returns the compiled module ready for eval.
///
/// Identical to [`parse_and_compile`] except uses `reify_compiler::compile_with_stdlib`
/// so that stdlib types and traits are available during compilation, and routes
/// the parse step through `reify_compiler::parse_with_stdlib` (via
/// [`compile_source_with_stdlib`]) so prelude-stdlib enum names participate
/// in `EnumAccess` disambiguation.
///
/// # Panics
/// Panics if there are any parse errors or error-severity compile diagnostics.
pub fn parse_and_compile_with_stdlib(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source_with_stdlib(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

/// Parse `source`, compile, assert ≥1 Error-severity diagnostic is produced.
/// If `needle` is non-empty, also assert at least one error message contains it.
/// Returns the `CompiledModule` for optional further assertions.
///
/// # Panics
/// Panics if there are parse errors, if no compile errors are produced, or
/// if `needle` is non-empty and no error message contains it.
pub fn parse_compile_expect_err(source: &str, needle: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(!errors.is_empty(), "expected at least one compile error");
    if !needle.is_empty() {
        assert!(
            errors.iter().any(|d| d.message.contains(needle)),
            "expected error containing {:?}, got: {:?}",
            needle,
            errors
        );
    }
    compiled
}

/// Assert that `diagnostics` contains at least one entry whose severity equals
/// `severity` and whose message contains `contains`.
///
/// Use this to verify that a specific diagnostic was emitted — for example, to
/// confirm that a particular error or warning appears after a compile step.
///
/// # Panics
/// Panics if no diagnostic matches both `severity` and `contains`. The panic
/// message includes the full `diagnostics` list for debugging.
#[track_caller]
pub fn assert_has_diagnostic(diagnostics: &[Diagnostic], severity: Severity, contains: &str) {
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == severity && d.message.contains(contains)),
        "expected diagnostic with severity={:?} containing {:?}, got: {:?}",
        severity,
        contains,
        diagnostics
    );
}

/// Assert that `diagnostics` contains no entry whose severity equals `severity`
/// and whose message contains `contains`.
///
/// Use this as a negative assertion — for example, to confirm that a specific
/// warning was suppressed, or that a particular error was not emitted.
///
/// # Panics
/// Panics if any diagnostic matches both `severity` and `contains`. The panic
/// message includes the matching diagnostics so it's clear which ones violated
/// the assertion.
#[track_caller]
pub fn assert_no_diagnostic(diagnostics: &[Diagnostic], severity: Severity, contains: &str) {
    let matched: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == severity && d.message.contains(contains))
        .collect();
    assert!(
        matched.is_empty(),
        "expected no diagnostic with severity={:?} containing {:?}, got: {:?}",
        severity,
        contains,
        matched
    );
}

/// Assert that `diagnostics` contains no `Severity::Error` entries.
///
/// `context` is a short label that appears in the panic message to identify
/// which compilation or evaluation phase failed — e.g. `"compile"`, `"eval"`,
/// or `"post-link"`. This is useful when a single test exercises multiple
/// pipeline stages and you need to identify which one produced errors.
///
/// Warnings, Info, and other non-Error severities are allowed and do not
/// cause a panic. Use [`assert_no_diagnostics`] instead when all severities
/// must be absent.
///
/// # Panics
/// Panics if any `Severity::Error` diagnostic is present. The panic message
/// includes `context` and the list of error messages.
#[track_caller]
pub fn assert_no_error_diagnostics(diagnostics: &[Diagnostic], context: &str) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{context}: expected no error diagnostics, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Assert that `diagnostics` is completely empty — no diagnostics of any severity.
///
/// This is stricter than [`assert_no_error_diagnostics`]: it fails on Warnings,
/// Info, and any other severity in addition to Errors. Use this for
/// characterization tests where the intent is "absolutely nothing is emitted".
///
/// `context` is a short label that appears in the panic message to identify
/// which compilation or evaluation phase failed — e.g. `"compile"`, `"guard block"`.
///
/// # Panics
/// Panics if `diagnostics` is non-empty. The panic message includes `context`
/// and the full list of diagnostic messages.
#[track_caller]
pub fn assert_no_diagnostics(diagnostics: &[Diagnostic], context: &str) {
    assert!(
        diagnostics.is_empty(),
        "{context}: expected no diagnostics at all, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Run a full geometry pipeline for a given [`reify_compiler::ModifyKind`].
///
/// Creates a module with 2 compiled ops:
///
/// - Op 0: `Box` primitive at 20 mm × 20 mm × 20 mm
/// - Op 1: `Modify` with `kind`, `target: GeomRef::Step(0)`, and `modify_args`
///
/// Runs the `Engine` with a fresh `MockConstraintChecker` and `MockGeometryKernel`,
/// then asserts:
///
/// - No error diagnostics were produced
/// - `result.geometry_output` is `Some` (geometry was emitted)
///
/// Returns the [`reify_eval::BuildResult`] and the recorded
/// [`crate::mocks::GeometryOpRecord`]s as owned values.
///
/// # Example
///
/// ```ignore
/// let (result, ops) = run_modify_pipeline(
///     ModifyKind::Chamfer,
///     vec![("distance".into(), CompiledExpr::literal(mm(3.0), Type::length()))],
/// );
/// assert_eq!(ops.len(), 2);
/// assert!(matches!(ops[1].op, GeometryOp::Chamfer { .. }));
/// ```
///
/// # Panics
///
/// Panics if the build produces error diagnostics or no geometry output.
#[cfg(feature = "eval-helpers")]
#[track_caller]
pub fn run_modify_pipeline(
    kind: reify_compiler::ModifyKind,
    modify_args: Vec<(String, reify_ir::CompiledExpr)>,
) -> (reify_eval::BuildResult, Vec<crate::mocks::GeometryOpRecord>) {
    use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind};
    use reify_core::Type;
    use reify_ir::ExportFormat;

    let mm_literal =
        |v: f64| reify_ir::CompiledExpr::literal(crate::values::mm(v), Type::length());

    let entity_name = format!("Test{kind:?}");

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(20.0)),
            ("height".into(), mm_literal(20.0)),
            ("depth".into(), mm_literal(20.0)),
        ],
    };

    let modify_op = CompiledGeometryOp::Modify {
        kind,
        target: GeomRef::Step(0),
        args: modify_args,
    };

    let template = crate::builders::TopologyTemplateBuilder::new(&entity_name)
        .realization(&entity_name, 0, vec![box_op, modify_op])
        .build();

    let module = crate::builders::CompiledModuleBuilder::new(reify_core::ModulePath::single(
        format!("test_{}", entity_name.to_lowercase()),
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_no_error_diagnostics(&result.diagnostics, "run_modify_pipeline");
    assert!(
        result.geometry_output.is_some(),
        "engine should produce geometry output"
    );

    let ops = ops_ref.lock().unwrap().clone();
    (result, ops)
}

/// Retrieve the compiled `default_expr` of any value cell by name from a named template.
///
/// Resolves any value cell carrying a `default_expr` — `let` bindings and defaulted
/// `param`s alike — since lookup keys on the cell's member name, not its kind. A
/// defaultless cell (e.g. an `auto` param) panics; see # Panics.
///
/// Variant of [`get_let_expr`] for multi-structure modules where `templates.first()` may
/// not be the desired template. `get_let_expr` delegates to this function.
///
/// # Panics
/// - `"no template named '{template_name}'"` if no template with that name exists.
/// - `"no value cell named '{cell_name}' in template '{template_name}'"` if the cell is absent.
/// - `"value cell '{cell_name}' in '{template_name}' has no default expr"` if `default_expr` is `None`.
pub fn get_let_expr_in<'a>(
    module: &'a reify_compiler::CompiledModule,
    template_name: &str,
    cell_name: &str,
) -> &'a CompiledExpr {
    let template = module
        .templates
        .iter()
        .find(|t| t.name == template_name)
        .unwrap_or_else(|| panic!("no template named '{template_name}'"));
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == cell_name)
        .unwrap_or_else(|| {
            panic!("no value cell named '{cell_name}' in template '{template_name}'")
        });
    cell.default_expr.as_ref().unwrap_or_else(|| {
        panic!("value cell '{cell_name}' in '{template_name}' has no default expr")
    })
}

/// Retrieve the compiled `default_expr` of any value cell by name from the first template.
///
/// Resolves any value cell carrying a `default_expr` — `let` bindings and defaulted
/// `param`s alike — since lookup keys on the cell's member name, not its kind. A
/// defaultless cell (e.g. an `auto` param) panics; see # Panics.
///
/// Convenience wrapper that delegates to [`get_let_expr_in`] using the name of the first
/// template in the module. Use [`get_let_expr_in`] directly when the module has multiple
/// templates and you need to target a specific one.
///
/// # Panics
/// - `"expected at least one template in module"` if `templates` is empty.
/// - Panics from [`get_let_expr_in`] if the cell or its default expr is absent.
pub fn get_let_expr<'a>(
    module: &'a reify_compiler::CompiledModule,
    name: &str,
) -> &'a CompiledExpr {
    let template_name = module
        .templates
        .first()
        .expect("expected at least one template in module")
        .name
        .as_str();
    get_let_expr_in(module, template_name, name)
}

/// Assert the anti-cascade contract: exactly the expected root-cause error(s) are present
/// and no unexpected additional errors appear.
///
/// # Parameters
/// - `diagnostics`: All diagnostics from the compiled module.
/// - `expected_root_fragments`: One or more substrings.  At least one
///   `Severity::Error` diagnostic must contain at least one of the fragments
///   (the root-cause error).  EVERY error must match at least one fragment;
///   any error that matches none is treated as an unexpected cascade error and
///   causes the assertion to fail.
///
/// ## Multi-fragment use case
/// When a single compilation triggers more than one legitimate root-cause error
/// (e.g. "duplicate function signature" and "ambiguous function call"), pass all
/// expected fragments as a slice: `&["ambiguous", "duplicate"]`.  This avoids
/// an inline cascade check and keeps all cascade assertions in one place.
///
/// ## Rationale
/// The previous approach (`!message.contains("mismatch") || !message.contains("incompatible")`)
/// is fragile: it misses cascade diagnostics with different wording.  The positive-whitelist
/// approach here fails on ANY unexpected error, making it both stricter and easier to maintain
/// from a single definition.
///
/// ## Fragment selection
/// Each fragment is matched using [`str::contains`], so a short or common word like
/// `"ambiguous"` would also match an unrelated diagnostic that happens to contain it,
/// silently treating that unrelated error as an expected root-cause and weakening the
/// whitelist.  Callers should therefore prefer specific, distinctive phrases
/// (e.g. `"unknown member"`, `"duplicate function signature"`, `"ambiguous function call"`)
/// over standalone common English words.  If a fragment collision ever surfaces and causes
/// a spurious assertion pass, the heavier alternative — accepting a predicate
/// (`impl Fn(&str) -> bool`) instead of `&[&str]` — can be revisited at that point.
///
/// # Panics
/// - `"expected root-cause error matching one of ..."` if no error matches any fragment.
/// - `"unexpected cascade errors ..."` if any error matches no fragment.
#[track_caller]
pub fn assert_no_type_cascade(diagnostics: &[Diagnostic], expected_root_fragments: &[&str]) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let matches_any = |msg: &str| expected_root_fragments.iter().any(|f| msg.contains(f));

    assert!(
        errors.iter().any(|d| matches_any(&d.message)),
        "expected root-cause error matching one of {expected_root_fragments:?}; got: {errors:?}",
    );

    let unexpected: Vec<_> = errors.iter().filter(|d| !matches_any(&d.message)).collect();

    assert!(
        unexpected.is_empty(),
        "unexpected cascade errors (must all match one of {expected_root_fragments:?}): {unexpected:?}",
    );
}

/// Compute the axis-aligned bounding box of a tessellated [`reify_ir::Mesh`]
/// by scanning its flat `[x0,y0,z0,x1,y1,z1,...]` vertex buffer.
///
/// Shared replacement for the private `mesh_aabb` copies duplicated across
/// `reify-eval`'s e2e tests (task 4959; surfaced by task 3963 code review).
///
/// Uses `chunks_exact(3)` to walk the buffer, so if `mesh.vertices.len()` is
/// not a multiple of 3 the trailing partial vertex is silently ignored (see
/// `test_mesh_aabb_ignores_trailing_partial_vertex`).
///
/// # Panics
/// Panics if `mesh.vertices` is empty.
pub fn mesh_aabb(mesh: &reify_ir::Mesh) -> ([f32; 3], [f32; 3]) {
    assert!(!mesh.vertices.is_empty(), "mesh_aabb: vertex buffer is empty");
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in mesh.vertices.chunks_exact(3) {
        for axis in 0..3 {
            min[axis] = min[axis].min(v[axis]);
            max[axis] = max[axis].max(v[axis]);
        }
    }
    (min, max)
}

/// Look up the `Value` of a value cell by `(structure, member)` from an
/// [`reify_eval::EvalResult`]. Canonical replacement for the verbatim-duplicated
/// `cell_value` helper in `result_recovery_e2e.rs`, `result_fallback_e2e.rs`, and
/// `fallback_recovery_e2e.rs`, and the generalization of `parse_length_e2e.rs`'s
/// 1-arg variant (task 5182 / esc-4039-1).
/// # Panics
/// Panics if no cell named `{structure}.{member}` exists; the message lists
/// available cell names.
#[cfg(feature = "eval-helpers")]
#[track_caller]
pub fn cell_value(result: &reify_eval::EvalResult, structure: &str, member: &str) -> reify_ir::Value {
    let id = reify_core::ValueCellId::new(structure, member);
    result.values.get(&id).cloned().unwrap_or_else(|| {
        panic!(
            "{structure}.{member} not found in eval result; available: {:?}",
            result.values.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>()
        )
    })
}

/// Sorted `member` list of every cell `result` produced for `entity` — used to
/// make a missing-cell panic read as "mirror reintroduced" rather than
/// "cell renamed".
///
/// Canonical replacement for the verbatim-duplicated `members_of` helper in
/// `m8_3_stdlib_integration.rs` and `m8_4_stdlib_integration.rs` (task #5653;
/// surfaced by task #5582 code review).
#[cfg(feature = "eval-helpers")]
pub fn members_of(result: &reify_eval::EvalResult, entity: &str) -> Vec<String> {
    let mut members: Vec<String> = result
        .values
        .iter()
        .filter(|(id, _)| id.entity == entity)
        .map(|(id, _)| id.member.clone())
        .collect();
    members.sort();
    members
}

#[cfg(test)]
mod tests {
    use crate::fixtures::bracket_source;
    use reify_core::{Diagnostic, Severity};

    /// Build a `reify_eval::EvalResult` from the only two fields these unit
    /// tests ever vary. `EvalResult` derives no `Default`, so keeping its
    /// 5-field literal in exactly ONE place means a new field on the struct is
    /// a one-line fix here rather than an edit at every test site
    /// (task #5653 review).
    #[cfg(feature = "eval-helpers")]
    fn eval_result(
        values: reify_ir::ValueMap,
        diagnostics: Vec<reify_core::Diagnostic>,
    ) -> reify_eval::EvalResult {
        reify_eval::EvalResult {
            values,
            diagnostics,
            resolved_params: std::collections::HashMap::new(),
            objective_provenance: std::collections::HashMap::new(),
            structured_detail: vec![],
        }
    }

    /// `eval_result`'s sibling for `reify_eval::CheckResult` — the
    /// `assert_no_check_errors` tests below vary only `diagnostics`, so the same
    /// one-place-to-edit rationale as `eval_result` applies: `CheckResult`
    /// derives no `Default`, and open-coding its 5-field literal at each
    /// `#[should_panic]` site would make a new struct field an edit at every one
    /// of them (task #5653 review).
    #[cfg(feature = "eval-helpers")]
    fn check_result(diagnostics: Vec<reify_core::Diagnostic>) -> reify_eval::CheckResult {
        reify_eval::CheckResult {
            values: reify_ir::ValueMap::new(),
            constraint_results: vec![],
            diagnostics,
            resolved_params: std::collections::HashMap::new(),
            structured_detail: vec![],
        }
    }

    /// mesh_aabb: computes the correct (min, max) AABB over a known flat
    /// vertex buffer.
    #[test]
    fn test_mesh_aabb_known_bounds() {
        let mesh = reify_ir::Mesh {
            vertices: vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, -1.0, -2.0, -3.0],
            indices: vec![],
            normals: None,
        };
        let (min, max) = super::mesh_aabb(&mesh);
        assert_eq!(min, [-1.0, -2.0, -3.0], "unexpected min; got {min:?}");
        assert_eq!(max, [1.0, 2.0, 3.0], "unexpected max; got {max:?}");
    }

    /// mesh_aabb: panics on a mesh with an empty vertex buffer.
    #[test]
    #[should_panic(expected = "vertex buffer is empty")]
    fn test_mesh_aabb_empty_vertices_panics() {
        let mesh = reify_ir::Mesh {
            vertices: vec![],
            indices: vec![],
            normals: None,
        };
        let _ = super::mesh_aabb(&mesh);
    }

    /// mesh_aabb: a vertex buffer whose length is not a multiple of 3 is
    /// tolerated by silently ignoring the trailing partial vertex, since the
    /// scan uses `chunks_exact(3)`. Pins down this documented tolerance so a
    /// future change to the scan strategy doesn't shift it unnoticed.
    #[test]
    fn test_mesh_aabb_ignores_trailing_partial_vertex() {
        let mesh = reify_ir::Mesh {
            // Trailing lone `99.0` is a malformed partial vertex; it must not
            // affect min/max (e.g. by leaking into axis 0 of a phantom vertex).
            vertices: vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 99.0],
            indices: vec![],
            normals: None,
        };
        let (min, max) = super::mesh_aabb(&mesh);
        assert_eq!(min, [0.0, 0.0, 0.0], "unexpected min; got {min:?}");
        assert_eq!(max, [1.0, 2.0, 3.0], "unexpected max; got {max:?}");
    }

    /// cell_value: returns the `Value` for a present `(structure, member)` cell.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn cell_value_returns_value_for_present_cell() {
        use reify_ir::ValueMap;
        let mut values = ValueMap::new();
        values.insert(reify_core::ValueCellId::new("S", "x"), reify_ir::Value::Bool(true));
        let result = eval_result(values, vec![]);
        assert_eq!(super::cell_value(&result, "S", "x"), reify_ir::Value::Bool(true));
    }

    /// cell_value: panics (message naming the missing cell) when no cell named
    /// `{structure}.{member}` is present in the result's `ValueMap`.
    #[cfg(feature = "eval-helpers")]
    #[test]
    #[should_panic(expected = "not found in eval result")]
    fn cell_value_panics_on_absent_cell() {
        use reify_ir::ValueMap;
        let result = eval_result(ValueMap::new(), vec![]);
        let _ = super::cell_value(&result, "Nope", "missing");
    }

    /// members_of: the returned member list is sorted ascending, independent of
    /// the order the cells were inserted into the `ValueMap` (pins the explicit
    /// `members.sort()`, so the helper never leaks `ValueMap` iteration order
    /// into a panic message).
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn members_of_returns_sorted_members() {
        use reify_ir::ValueMap;
        let mut values = ValueMap::new();
        // Inserted in deliberately NON-ascending member order.
        values.insert(
            reify_core::ValueCellId::new("Flange.pos", "zone_shape"),
            reify_ir::Value::Bool(true),
        );
        values.insert(
            reify_core::ValueCellId::new("Flange.pos", "nominal_zone"),
            reify_ir::Value::Bool(true),
        );
        values.insert(
            reify_core::ValueCellId::new("Flange.pos", "feature"),
            reify_ir::Value::Bool(true),
        );
        let result = eval_result(values, vec![]);
        let members = super::members_of(&result, "Flange.pos");
        assert_eq!(
            members,
            vec!["feature", "nominal_zone", "zone_shape"],
            "members must come back sorted ascending; got {members:?}",
        );
    }

    /// members_of: only cells belonging to the queried entity are reported —
    /// a same-named member on a *different* entity must not leak in (pins the
    /// `id.entity == entity` filter).
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn members_of_filters_other_entities() {
        use reify_ir::ValueMap;
        let mut values = ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Flange.pos", "a"),
            reify_ir::Value::Bool(true),
        );
        values.insert(
            reify_core::ValueCellId::new("Flange.flat", "b"),
            reify_ir::Value::Bool(true),
        );
        let result = eval_result(values, vec![]);
        let members = super::members_of(&result, "Flange.pos");
        assert_eq!(
            members,
            vec!["a"],
            "only Flange.pos's own members may be reported; got {members:?}",
        );
    }

    /// members_of: the entity filter is EXACT equality, not a prefix/ancestor
    /// match — a cell on the descendant entity `Flange.pos` must not be
    /// reported for the query `Flange`.
    ///
    /// This is the case the call sites actually depend on: `m8_3_stdlib_integration`
    /// asserts `members_of(&result, root)` is empty for the bare root names
    /// `Position` / `Flatness` / … to prove no standalone mirror ROOT entity was
    /// reintroduced. Under prefix semantics that assertion would instead fire on
    /// any entity merely *starting with* the root name, silently changing what it
    /// detects — and the sibling-only case above (`Flange.pos` vs `Flange.flat`)
    /// would not catch the swap, since neither name prefixes the other.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn members_of_matches_entity_exactly_not_by_prefix() {
        use reify_ir::ValueMap;
        let mut values = ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Flange", "own"),
            reify_ir::Value::Bool(true),
        );
        values.insert(
            reify_core::ValueCellId::new("Flange.pos", "descendant"),
            reify_ir::Value::Bool(true),
        );
        let result = eval_result(values, vec![]);
        let members = super::members_of(&result, "Flange");
        assert_eq!(
            members,
            vec!["own"],
            "querying `Flange` must report only its OWN members, never the \
             descendant entity `Flange.pos`'s — the filter is `==`, not \
             `starts_with`; got {members:?}",
        );
    }

    /// members_of: an entity with no cells yields an empty `Vec` rather than
    /// panicking — the contract difference from its neighbour `cell_value`,
    /// which panics on a missing cell. Every call site invokes `members_of`
    /// *inside* a panic-message formatter, so a panic here would mask the
    /// caller's own diagnostic.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn members_of_returns_empty_for_unknown_entity() {
        use reify_ir::ValueMap;
        let mut values = ValueMap::new();
        values.insert(
            reify_core::ValueCellId::new("Flange.pos", "a"),
            reify_ir::Value::Bool(true),
        );
        let result = eval_result(values, vec![]);
        let members = super::members_of(&result, "NoSuchEntity");
        assert!(
            members.is_empty(),
            "an unknown entity must yield an empty list, not panic; got {members:?}",
        );
    }

    /// assert_no_eval_errors should not panic when the result has no diagnostics.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_assert_no_eval_errors_passes_on_clean_result() {
        use reify_ir::ValueMap;
        let result = eval_result(ValueMap::new(), vec![]);
        super::assert_no_eval_errors(&result);
    }

    /// assert_no_eval_errors should panic (with message containing "eval errors")
    /// when the result contains at least one Error-severity diagnostic.
    #[cfg(feature = "eval-helpers")]
    #[test]
    #[should_panic(expected = "eval errors")]
    fn test_assert_no_eval_errors_panics_on_error_diagnostic() {
        use reify_ir::ValueMap;
        let result = eval_result(ValueMap::new(), vec![Diagnostic::error("something went wrong")]);
        super::assert_no_eval_errors(&result);
    }

    /// assert_no_check_errors should not panic when the CheckResult has no diagnostics.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_assert_no_check_errors_passes_on_clean_result() {
        let result = check_result(vec![]);
        super::assert_no_check_errors(&result);
    }

    /// assert_no_check_errors should panic (with message containing "check errors")
    /// when the CheckResult contains at least one Error-severity diagnostic.
    #[cfg(feature = "eval-helpers")]
    #[test]
    #[should_panic(expected = "check errors")]
    fn test_assert_no_check_errors_panics_on_error_diagnostic() {
        let result = check_result(vec![Diagnostic::error("something went wrong")]);
        super::assert_no_check_errors(&result);
    }

    /// assert_no_check_errors should not panic when the CheckResult has only warnings
    /// (no Error-severity diagnostics).
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_assert_no_check_errors_passes_with_warnings_only() {
        let result = check_result(vec![Diagnostic::warning("just a warning")]);
        // Should not panic — warnings are not errors
        super::assert_no_check_errors(&result);
    }

    /// assert_no_eval_errors should not panic when the result has only warnings
    /// (no Error-severity diagnostics).
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_assert_no_eval_errors_ignores_warnings() {
        use reify_ir::ValueMap;
        let result = eval_result(ValueMap::new(), vec![Diagnostic::warning("just a warning")]);
        // Should not panic — warnings are not errors
        super::assert_no_eval_errors(&result);
    }

    /// assert_eval_clean should not panic when the result has no diagnostics.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_assert_eval_clean_passes_on_empty_result() {
        use reify_ir::ValueMap;
        let result = eval_result(ValueMap::new(), vec![]);
        super::assert_eval_clean(&result);
    }

    /// assert_eval_clean should panic when the result has a Warning diagnostic —
    /// it is stricter than assert_no_eval_errors.
    #[cfg(feature = "eval-helpers")]
    #[test]
    #[should_panic(expected = "expected no diagnostics")]
    fn test_assert_eval_clean_panics_on_warning() {
        use reify_ir::ValueMap;
        let result = eval_result(ValueMap::new(), vec![Diagnostic::warning("just a warning")]);
        super::assert_eval_clean(&result);
    }

    #[test]
    fn test_compile_source_valid() {
        let compiled = super::compile_source(bracket_source());
        assert!(
            !compiled.templates.is_empty(),
            "compile_source should produce at least one template for bracket source"
        );
    }

    #[test]
    fn test_compile_source_with_errors() {
        // Source with an undefined reference — compile_source should NOT panic,
        // instead it returns the module WITH error diagnostics.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let compiled = super::compile_source(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "compile_source with invalid source should produce error diagnostics"
        );
    }

    #[test]
    fn test_compile_source_with_stdlib() {
        // Source referencing stdlib trait MaterialSpec — should compile without errors
        // only when stdlib is loaded.
        let source = r#"structure Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name: String = "Steel"
        }"#;
        let compiled = super::compile_source_with_stdlib(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "compile_source_with_stdlib should compile stdlib-dependent source without errors: {:?}",
            errors
        );
    }

    /// Task 2525: `compile_source_with_stdlib` must route the parse step
    /// through `reify_compiler::parse_with_stdlib` so that prelude/stdlib
    /// enum names participate in the parser's `EnumAccess` disambiguation
    /// pass.  A source referencing `CorrosionClass.C5` WITHOUT an inline
    /// `enum CorrosionClass` declaration must therefore compile cleanly.
    ///
    /// Fails today (the helper still uses the prelude-blind `parse`); pins
    /// the contract that step-8 wires the helper through `parse_with_stdlib`.
    #[test]
    fn compile_source_with_stdlib_resolves_prelude_enum_access() {
        let source = r#"structure def CorrTest : CorrosionResistant {
            param density : Density = 7850kg/m^3
            param name : String = "test_steel"
            param corrosion_class : CorrosionClass = CorrosionClass.C5
        }"#;

        let compiled = super::compile_source_with_stdlib(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "compile_source_with_stdlib should resolve CorrosionClass.C5 against the stdlib prelude (no inline redecl), got errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_compile_first_template() {
        let (template, diagnostics) = super::compile_first_template(bracket_source());
        assert_eq!(template.name, "Bracket", "first template should be Bracket");
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn test_compile_template_by_name() {
        let source = r#"
            structure Alpha { param x: Length = 1 }
            structure Beta { param y: Length = 2 }
        "#;
        let (template, _diags) = super::compile_template(source, "Beta");
        assert_eq!(template.name, "Beta", "should extract template named Beta");
    }

    #[test]
    #[should_panic(expected = "not found")]
    fn test_compile_template_panics_on_missing_name() {
        super::compile_template(bracket_source(), "NonExistent");
    }

    #[test]
    fn test_collect_errors_filters_correctly() {
        // Source with an undefined reference produces Error diagnostics.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let compiled = super::compile_source(source);
        let errors = super::collect_errors(&compiled.diagnostics);
        assert!(
            !errors.is_empty(),
            "collect_errors should return error diagnostics for invalid source"
        );
        // All returned diagnostics must be Error severity.
        for d in &errors {
            assert_eq!(
                d.severity,
                Severity::Error,
                "collect_errors returned non-Error: {:?}",
                d
            );
        }
    }

    #[test]
    fn test_errors_only_convenience() {
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        let compiled = super::compile_source(source);
        let errors = super::errors_only(&compiled);
        assert!(
            !errors.is_empty(),
            "errors_only should return error diagnostics for invalid source"
        );
    }

    /// error_diags should return only the `Severity::Error` entries from a
    /// mixed-severity slice, preserving referential identity.
    #[test]
    fn error_diags_filters_errors_from_mixed_severities() {
        let diags = vec![
            Diagnostic::error("first error"),
            Diagnostic::warning("a warning"),
            Diagnostic::info("an info note"),
        ];
        let errors = super::error_diags(&diags);
        assert_eq!(
            errors.len(),
            1,
            "error_diags should return exactly one entry for this fixture"
        );
        assert_eq!(
            errors[0].severity,
            Severity::Error,
            "the single returned diagnostic must have Severity::Error"
        );
        assert_eq!(
            errors[0].message, "first error",
            "returned diagnostic message should match the original Error entry"
        );
    }

    /// error_diags should return an empty Vec for an empty input slice.
    #[test]
    fn error_diags_empty_slice_returns_empty() {
        let diags: Vec<Diagnostic> = vec![];
        assert!(super::error_diags(&diags).is_empty());
    }

    /// error_diags should return an empty Vec when the input contains only
    /// non-Error diagnostics (warning-only input).
    #[test]
    fn error_diags_warning_only_returns_empty() {
        let diags = vec![Diagnostic::warning("just a warning")];
        assert!(super::error_diags(&diags).is_empty());
    }

    #[test]
    fn test_warnings_only_filters_correctly() {
        // Use warn_source_with_unknown_port_type which produces warnings.
        let source = crate::fixtures::warn_source_with_unknown_port_type();
        let compiled = super::compile_source(source);
        let warnings = super::warnings_only(&compiled);
        assert!(
            !warnings.is_empty(),
            "warnings_only should return warning diagnostics for warn source"
        );
        for d in &warnings {
            assert_eq!(
                d.severity,
                Severity::Warning,
                "warnings_only returned non-Warning: {:?}",
                d
            );
        }
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_eval_source() {
        let result = super::eval_source(bracket_source());
        assert!(
            !result.values.is_empty(),
            "eval_source should produce non-empty values for bracket source"
        );
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_check_source() {
        let result = super::check_source(bracket_source());
        assert!(
            !result.constraint_results.is_empty(),
            "check_source should produce non-empty constraint_results for bracket source"
        );
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_check_source_with_stdlib() {
        // Source referencing stdlib trait MaterialSpec with a constraint.
        let source = r#"structure Steel : MaterialSpec {
            param density : Density = 7850kg/m^3
            param name: String = "Steel"
            constraint density > 0
        }"#;
        let result = super::check_source_with_stdlib(source);
        assert!(
            !result.constraint_results.is_empty(),
            "check_source_with_stdlib should produce constraint_results"
        );
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_make_engine() {
        // Use a simple non-geometry source to avoid coupling to bracket fixture shape.
        let source = "structure S { param x: Length = 42 }";
        let compiled = super::parse_and_compile(source);
        let mut engine = super::make_engine();
        let result = engine.eval(&compiled);
        assert!(
            !result.values.is_empty(),
            "engine.eval should produce non-empty values"
        );
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_make_simple_engine() {
        use reify_ir::Satisfaction;
        let compiled = super::parse_and_compile(bracket_source());
        let mut engine = super::make_simple_engine();
        let result = engine.check(&compiled);
        assert!(
            !result.constraint_results.is_empty(),
            "engine.check should produce non-empty constraint_results for bracket source"
        );
        for entry in &result.constraint_results {
            assert_eq!(
                entry.satisfaction,
                Satisfaction::Satisfied,
                "constraint {} should be Satisfied under SimpleConstraintChecker, got {:?}",
                entry.id,
                entry.satisfaction
            );
        }
    }

    /// Negative test: a constraint that is definitively false should produce
    /// `Satisfaction::Violated` under `SimpleConstraintChecker`, differentiating
    /// it from `MockConstraintChecker` (which only tracks, never really evaluates).
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_make_simple_engine_violated_constraint() {
        use reify_ir::Satisfaction;

        let source = r#"structure Bad {
            param a: Real = 1.0
            constraint a > 2.0
        }"#;

        let result = super::check_source(source);

        // Must produce exactly 1 constraint result
        assert_eq!(
            result.constraint_results.len(),
            1,
            "expected exactly 1 constraint result, got {}",
            result.constraint_results.len()
        );

        // That constraint must be Violated (1.0 > 2.0 is false)
        assert_eq!(
            result.constraint_results[0].satisfaction,
            Satisfaction::Violated,
            "constraint should be Violated (1.0 > 2.0 is false), got {:?}",
            result.constraint_results[0].satisfaction
        );
    }

    #[test]
    fn test_parse_compile_expect_err_detects_error() {
        // Source with an undefined reference should produce a compile error.
        let source = r#"structure Bad {
            let x = unknown_variable
        }"#;
        // Should not panic — the function expects errors.
        let _compiled = super::parse_compile_expect_err(source, "");
    }

    #[test]
    fn test_parse_compile_expect_err_needle_match() {
        // Use a controlled needle that matches a specific known error message.
        let source = r#"structure Bad {
            let x = totally_undefined_name
        }"#;
        let _compiled = super::parse_compile_expect_err(source, "totally_undefined_name");
    }

    #[test]
    fn test_parse_and_compile_with_stdlib() {
        // Use a simple source that requires stdlib (MaterialSpec trait) without
        // coupling to specific stdlib field values or shape details.
        let source = r#"structure S : MaterialSpec {
            param density : Density = 1kg/m^3
            param name: String = "S"
        }"#;
        // parse_and_compile_with_stdlib panics on errors, so reaching here means success.
        let compiled = super::parse_and_compile_with_stdlib(source);
        assert!(
            !compiled.templates.is_empty(),
            "should produce at least one template"
        );
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_check_source_satisfied() {
        use reify_ir::Satisfaction;
        let result = super::check_source(bracket_source());
        assert!(
            !result.constraint_results.is_empty(),
            "check_source should produce non-empty constraint_results for bracket source"
        );
        for entry in &result.constraint_results {
            assert_eq!(
                entry.satisfaction,
                Satisfaction::Satisfied,
                "constraint {} should be Satisfied via check_source",
                entry.id,
            );
        }
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    #[should_panic(expected = "parse errors")]
    fn test_eval_source_panics_on_invalid_source() {
        super::eval_source("not valid {");
    }

    #[cfg(feature = "eval-helpers")]
    #[test]
    #[should_panic(expected = "parse errors")]
    fn test_check_source_panics_on_invalid_source() {
        super::check_source("not valid {");
    }

    #[test]
    fn test_parse_and_compile_valid() {
        let compiled = super::parse_and_compile(bracket_source());
        let errors = super::collect_errors(&compiled.diagnostics);
        assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);
        assert!(
            !compiled.templates.is_empty(),
            "bracket source should produce at least one template"
        );
    }

    // ── assert_has_diagnostic ──────────────────────────────────────────────

    /// assert_has_diagnostic should not panic when the diagnostics slice contains
    /// an entry matching the requested severity and message substring.
    #[test]
    fn test_assert_has_diagnostic_passes_on_match() {
        let diags = vec![
            Diagnostic::warning("unused port x"),
            Diagnostic::error("type mismatch for y"),
        ];
        // Should not panic — there is an Error-severity entry containing "type mismatch".
        super::assert_has_diagnostic(&diags, Severity::Error, "type mismatch");
    }

    /// assert_has_diagnostic should panic when no diagnostic in the slice matches
    /// the requested severity + message substring.
    #[test]
    #[should_panic(expected = "expected diagnostic")]
    fn test_assert_has_diagnostic_panics_when_no_match() {
        let diags = vec![Diagnostic::warning("unused port x")];
        // Should panic — no Error-severity diagnostic exists.
        super::assert_has_diagnostic(&diags, Severity::Error, "type mismatch");
    }

    /// assert_has_diagnostic should panic when the message substring matches but
    /// the severity is wrong — confirming the severity filter applies in the
    /// positive-assertion path.
    #[test]
    #[should_panic(expected = "expected diagnostic")]
    fn test_assert_has_diagnostic_panics_when_wrong_severity() {
        let diags = vec![Diagnostic::warning("type mismatch")];
        // Should panic — the message matches but severity is Warning, not Error.
        super::assert_has_diagnostic(&diags, Severity::Error, "type mismatch");
    }

    // ── assert_no_diagnostic ──────────────────────────────────────────────

    /// assert_no_diagnostic should not panic when the slice is empty.
    #[test]
    fn test_assert_no_diagnostic_passes_on_empty() {
        let diags: Vec<Diagnostic> = vec![];
        super::assert_no_diagnostic(&diags, Severity::Error, "anything");
    }

    /// assert_no_diagnostic should not panic when diagnostics exist but none match
    /// the requested severity + message substring.
    #[test]
    fn test_assert_no_diagnostic_passes_when_wrong_severity_or_message() {
        let diags = vec![
            Diagnostic::warning("type mismatch for x"),
            Diagnostic::error("unrelated error"),
        ];
        // Warning has the phrase but is wrong severity; Error has wrong message — no match.
        super::assert_no_diagnostic(&diags, Severity::Error, "type mismatch");
    }

    /// assert_no_diagnostic should panic when a diagnostic matches both severity
    /// and message substring.
    #[test]
    #[should_panic(expected = "expected no diagnostic")]
    fn test_assert_no_diagnostic_panics_on_match() {
        let diags = vec![Diagnostic::error("type mismatch for x")];
        // Should panic — an Error-severity diagnostic containing "type mismatch" exists.
        super::assert_no_diagnostic(&diags, Severity::Error, "type mismatch");
    }

    // ── assert_no_error_diagnostics ───────────────────────────────────────

    /// assert_no_error_diagnostics should not panic when the slice contains only
    /// Warning diagnostics (no Error-severity entries).
    #[test]
    fn test_assert_no_error_diagnostics_passes_with_only_warnings() {
        let diags = vec![
            Diagnostic::warning("unused port x"),
            Diagnostic::warning("deprecated syntax"),
        ];
        super::assert_no_error_diagnostics(&diags, "compile phase");
    }

    /// assert_no_error_diagnostics should not panic on an empty slice.
    #[test]
    fn test_assert_no_error_diagnostics_passes_on_empty() {
        let diags: Vec<Diagnostic> = vec![];
        super::assert_no_error_diagnostics(&diags, "eval phase");
    }

    /// assert_no_error_diagnostics should panic when an Error-severity diagnostic
    /// is present; the panic message must include the context label.
    #[test]
    #[should_panic(expected = "compile phase")]
    fn test_assert_no_error_diagnostics_panics_on_error() {
        let diags = vec![Diagnostic::error("undefined identifier")];
        super::assert_no_error_diagnostics(&diags, "compile phase");
    }

    // ── assert_no_diagnostics ─────────────────────────────────────────────

    /// assert_no_diagnostics should not panic when the slice is empty.
    #[test]
    fn test_assert_no_diagnostics_passes_on_empty() {
        let diags: Vec<Diagnostic> = vec![];
        super::assert_no_diagnostics(&diags, "guard compile");
    }

    /// assert_no_diagnostics should panic even on an Info-severity diagnostic;
    /// it is stricter than assert_no_error_diagnostics. The panic message must
    /// include the context label.
    #[test]
    #[should_panic(expected = "guard compile")]
    fn test_assert_no_diagnostics_panics_on_any_diagnostic() {
        let diags = vec![Diagnostic::info("informational note")];
        super::assert_no_diagnostics(&diags, "guard compile");
    }

    // ── get_let_expr_in ───────────────────────────────────────────────────

    /// get_let_expr_in should return the default_expr of the named cell in the
    /// named template, even when the module has multiple templates.
    /// Uses non-integer floats (1.5, 2.7) because whole-number float literals
    /// (e.g. 1.0, 2.0) are compiled as Type::Int by the Reify compiler when
    /// they satisfy `*v == (*v as i64) as f64`.
    #[test]
    fn test_get_let_expr_in_finds_named_template() {
        let source = r#"
            structure Alpha { let v = 1.5 }
            structure Beta  { let w = 2.7 }
        "#;
        let module = super::compile_source(source);
        let expr = super::get_let_expr_in(&module, "Beta", "w");
        assert_eq!(
            expr.result_type,
            reify_core::Type::dimensionless_scalar(),
            "expected result_type == Type::dimensionless_scalar() for Beta.w, got {:?}",
            expr.result_type
        );
    }

    /// get_let_expr_in should panic with "no template named" when the template
    /// name does not match any template in the module.
    #[test]
    #[should_panic(expected = "no template named")]
    fn test_get_let_expr_in_panics_on_missing_template() {
        let source = r#"structure S { let v = 1.0 }"#;
        let module = super::compile_source(source);
        super::get_let_expr_in(&module, "DoesNotExist", "v");
    }

    /// get_let_expr_in should panic with "no value cell named" when the cell
    /// name does not match any value cell in the named template.
    #[test]
    #[should_panic(expected = "no value cell named")]
    fn test_get_let_expr_in_panics_on_missing_cell() {
        let source = r#"structure S { let x = 1.0 }"#;
        let module = super::compile_source(source);
        super::get_let_expr_in(&module, "S", "y");
    }

    /// get_let_expr_in should panic with "has no default expr" for a value cell
    /// whose default_expr is None. Uses a builder-synthesized module with an
    /// auto_param (which always has default_expr = None) rather than a compiled
    /// source, since a source-level `param` always carries a default in well-formed
    /// compiled output.  The inline `assert!` below makes the precondition explicit:
    /// if `auto_param` ever changes to synthesize a placeholder default, the guard
    /// will fire loudly rather than silently letting the test pass for the wrong reason.
    #[test]
    #[should_panic(expected = "has no default expr")]
    fn test_get_let_expr_in_panics_on_missing_default_expr() {
        use reify_core::{ModulePath, Type};
        let template = crate::builders::TopologyTemplateBuilder::new("S")
            .auto_param("S", "x", Type::dimensionless_scalar())
            .build();
        // Precondition: auto_param must produce default_expr = None; if that ever
        // changes this guard fires before get_let_expr_in, surfacing the broken
        // assumption clearly instead of silently exercising the wrong branch.
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "x")
            .expect("auto_param should have added cell 'x'");
        assert!(
            cell.default_expr.is_none(),
            "auto_param must produce default_expr = None for this test's intent"
        );
        let module = crate::builders::CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();
        super::get_let_expr_in(&module, "S", "x");
    }

    /// get_let_expr_in resolves a defaulted `param` cell exactly like a `let`
    /// binding: lookup keys on `vc.id.member`, not cell kind, so lets and
    /// defaulted params resolve identically. Fixture mirrors reify-compiler's
    /// `constant_compile_tests::user_param_pi_shadows_builtin`.
    #[test]
    fn test_get_let_expr_in_resolves_defaulted_param_cell() {
        use reify_compiler::ValueCellKind;
        use reify_ir::{CompiledExprKind, Value};

        let source = "structure S {\n  param pi: Real = 1.5\n  let x = pi\n}";
        let module = super::compile_source(source);
        // Precondition: 'pi' really is a param cell carrying a default_expr,
        // not a let — otherwise this test would pass for the wrong reason.
        let cell = module
            .templates
            .first()
            .expect("expected at least one template")
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "pi")
            .expect("compiled module should have a value cell named 'pi'");
        assert_eq!(
            cell.kind,
            ValueCellKind::Param,
            "expected 'pi' to be a param cell, got {:?}",
            cell.kind
        );
        assert!(
            cell.default_expr.is_some(),
            "expected 'pi' param cell to carry a default_expr"
        );

        let expr = super::get_let_expr_in(&module, "S", "pi");
        match &expr.kind {
            CompiledExprKind::Literal(Value::Real(v)) => {
                assert!(
                    (*v - 1.5_f64).abs() < 1e-15,
                    "expected param default 1.5, got {}",
                    v
                );
            }
            other => panic!(
                "expected Literal(Real(1.5)) for defaulted param cell 'pi', got {:?}",
                other
            ),
        }
    }

    // ── get_let_expr ─────────────────────────────────────────────────────

    /// get_let_expr targets the FIRST template only; a cell in the second
    /// template is not reachable via get_let_expr.
    #[test]
    fn test_get_let_expr_uses_first_template_by_name() {
        let source = r#"
            structure Alpha { let a = 1.5 }
            structure Beta  { let b = 2.7 }
        "#;
        let module = super::compile_source(source);
        // Alpha is first — cell `a` should be found.
        let expr = super::get_let_expr(&module, "a");
        assert_eq!(expr.result_type, reify_core::Type::dimensionless_scalar());
    }

    /// get_let_expr with a cell name that only exists in the SECOND template
    /// should panic with "no value cell named", because the helper only looks
    /// inside the first template.
    #[test]
    #[should_panic(expected = "no value cell named")]
    fn test_get_let_expr_does_not_search_other_templates() {
        let source = r#"
            structure Alpha { let a = 1.5 }
            structure Beta  { let b = 2.7 }
        "#;
        let module = super::compile_source(source);
        // `b` is in Beta (second template), not Alpha (first) — must panic.
        super::get_let_expr(&module, "b");
    }

    /// get_let_expr should panic with "expected at least one template" when
    /// the module has no templates at all (empty module built via builder).
    #[test]
    #[should_panic(expected = "expected at least one template")]
    fn test_get_let_expr_panics_on_empty_templates() {
        use reify_core::ModulePath;
        let module =
            crate::builders::CompiledModuleBuilder::new(ModulePath::single("empty")).build();
        super::get_let_expr(&module, "anything");
    }

    /// get_let_expr (first-template convenience wrapper) resolves a defaulted
    /// `param` cell the same way it resolves a `let` binding. Together with
    /// `test_get_let_expr_in_resolves_defaulted_param_cell`, this pins the
    /// contract that these helpers are cell-KIND-agnostic: they key on
    /// `vc.id.member` and unwrap `default_expr`, so lets and defaulted params
    /// resolve identically.
    #[test]
    fn test_get_let_expr_resolves_defaulted_param_cell() {
        use reify_ir::{CompiledExprKind, Value};

        let source = "structure S {\n  param pi: Real = 1.5\n  let x = pi\n}";
        let module = super::compile_source(source);
        let expr = super::get_let_expr(&module, "pi");
        match &expr.kind {
            CompiledExprKind::Literal(Value::Real(v)) => {
                assert!(
                    (*v - 1.5_f64).abs() < 1e-15,
                    "expected param default 1.5, got {}",
                    v
                );
            }
            other => panic!(
                "expected Literal(Real(1.5)) for defaulted param cell 'pi', got {:?}",
                other
            ),
        }
    }

    // ── assert_no_type_cascade ────────────────────────────────────────────

    /// assert_no_type_cascade should not panic when the diagnostics slice
    /// contains at least one error matching a provided fragment, and ALL
    /// errors match at least one fragment.
    #[test]
    fn test_assert_no_type_cascade_passes_when_only_expected_errors() {
        let diags = vec![Diagnostic::error("unknown member 'x' on self")];
        super::assert_no_type_cascade(&diags, &["unknown member"]);
    }

    /// Warnings must be ignored by assert_no_type_cascade (it only filters
    /// to Severity::Error entries).
    #[test]
    fn test_assert_no_type_cascade_passes_with_mixed_severities() {
        let diags = vec![
            Diagnostic::error("unknown member 'x' on self"),
            Diagnostic::warning("unused port p"),
        ];
        super::assert_no_type_cascade(&diags, &["unknown member"]);
    }

    /// assert_no_type_cascade should panic with "expected root-cause error"
    /// when no error in the slice matches any of the expected fragments.
    #[test]
    #[should_panic(expected = "expected root-cause error")]
    fn test_assert_no_type_cascade_panics_when_no_root_cause() {
        let diags = vec![Diagnostic::error("parse error")];
        // "parse error" does not contain "unknown member", so assertion (a) fails.
        super::assert_no_type_cascade(&diags, &["unknown member"]);
    }

    /// assert_no_type_cascade should panic with "unexpected cascade errors"
    /// when at least one error does NOT match any expected fragment.
    #[test]
    #[should_panic(expected = "unexpected cascade errors")]
    fn test_assert_no_type_cascade_panics_on_unexpected_cascade() {
        let diags = vec![
            Diagnostic::error("unknown member 'x'"),
            Diagnostic::error("type mismatch for y"),
        ];
        // First matches, second doesn't → assertion (b) fails.
        super::assert_no_type_cascade(&diags, &["unknown member"]);
    }

    /// Multi-fragment whitelist: each error must match at least one fragment;
    /// assertion should not panic when all errors are covered.
    #[test]
    fn test_assert_no_type_cascade_multi_fragment_whitelist() {
        let diags = vec![
            Diagnostic::error("duplicate function signature"),
            Diagnostic::error("ambiguous function call"),
        ];
        // Both fragments are present; each error matches at least one.
        super::assert_no_type_cascade(&diags, &["ambiguous", "duplicate"]);
    }

    /// assert_no_type_cascade with an empty diagnostics slice should panic
    /// with "expected root-cause error" because no error matches any fragment.
    #[test]
    #[should_panic(expected = "expected root-cause error")]
    fn test_assert_no_type_cascade_panics_on_empty_diagnostics() {
        let diags: Vec<Diagnostic> = vec![];
        super::assert_no_type_cascade(&diags, &["anything"]);
    }

    // ── run_modify_pipeline smoke ─────────────────────────────────────────

    /// Smoke test for `run_modify_pipeline`: verifies the helper produces 2 ops
    /// and that ops[1].op matches the expected GeometryOp variant for both
    /// Chamfer and Fillet kinds.
    #[cfg(feature = "eval-helpers")]
    #[test]
    fn test_run_modify_pipeline_smoke() {
        use reify_compiler::ModifyKind;
        use reify_core::Type;
        use reify_ir::GeometryOp;
        let mm_literal =
            |v: f64| reify_ir::CompiledExpr::literal(crate::values::mm(v), Type::length());

        // Chamfer: expect 2 ops, ops[1] is GeometryOp::Chamfer
        let (_result, ops) = super::run_modify_pipeline(
            ModifyKind::Chamfer,
            vec![("distance".into(), mm_literal(3.0))],
        );
        assert_eq!(
            ops.len(),
            2,
            "expected 2 ops for Chamfer pipeline, got {}",
            ops.len()
        );
        assert!(
            matches!(ops[1].op, GeometryOp::Chamfer { .. }),
            "expected ops[1].op to be GeometryOp::Chamfer, got {:?}",
            ops[1].op
        );

        // Fillet: expect 2 ops, ops[1] is GeometryOp::Fillet
        let (_result, ops) = super::run_modify_pipeline(
            ModifyKind::Fillet,
            vec![("radius".into(), mm_literal(3.0))],
        );
        assert_eq!(
            ops.len(),
            2,
            "expected 2 ops for Fillet pipeline, got {}",
            ops.len()
        );
        assert!(
            matches!(ops[1].op, GeometryOp::Fillet { .. }),
            "expected ops[1].op to be GeometryOp::Fillet, got {:?}",
            ops[1].op
        );

        // Shell: expect 2 ops, ops[1] is GeometryOp::Shell
        // Shell only requires a `thickness` length arg; face indices are optional.
        // Draft and Thicken are not included here: Draft requires a plane handle
        // resolved from `step_handles.last()` (which in a 2-op pipeline is the Box
        // handle — valid, but the arg shape differs), and Thicken shares the same
        // structure as Shell without the face-list complexity.  Shell provides
        // sufficient additional coverage of the dispatch path.
        let (_result, ops) = super::run_modify_pipeline(
            ModifyKind::Shell,
            vec![("thickness".into(), mm_literal(2.0))],
        );
        assert_eq!(
            ops.len(),
            2,
            "expected 2 ops for Shell pipeline, got {}",
            ops.len()
        );
        assert!(
            matches!(ops[1].op, GeometryOp::Shell { .. }),
            "expected ops[1].op to be GeometryOp::Shell, got {:?}",
            ops[1].op
        );
    }

    // ─── collect_value_ref_members unit tests ────────────────────────────────

    /// collect_value_ref_members: bare ValueRef returns the member name.
    #[test]
    fn test_collect_value_ref_members_bare_value_ref() {
        use reify_core::Type;
        use reify_ir::CompiledExpr;

        let expr = CompiledExpr::value_ref(crate::vcid("E", "a"), Type::dimensionless_scalar());
        let result = super::collect_value_ref_members(&expr);
        assert!(
            result.iter().any(|m| m == "a"),
            "expected \"a\" in result; got {:?}",
            result
        );
    }

    /// collect_value_ref_members: BinOp recursion collects refs from both branches.
    #[test]
    fn test_collect_value_ref_members_binop() {
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr};

        let a = CompiledExpr::value_ref(crate::vcid("E", "a"), Type::dimensionless_scalar());
        let b = CompiledExpr::value_ref(crate::vcid("E", "b"), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Add, a, b, Type::dimensionless_scalar());
        let result = super::collect_value_ref_members(&expr);
        assert!(
            result.iter().any(|m| m == "a"),
            "expected \"a\" in result; got {:?}",
            result
        );
        assert!(
            result.iter().any(|m| m == "b"),
            "expected \"b\" in result; got {:?}",
            result
        );
    }

    /// collect_value_ref_members: UnOp recursion collects the operand's ref.
    #[test]
    fn test_collect_value_ref_members_unop() {
        use reify_core::Type;
        use reify_ir::{CompiledExpr, UnOp};

        let a = CompiledExpr::value_ref(crate::vcid("E", "a"), Type::dimensionless_scalar());
        let expr = CompiledExpr::unop(UnOp::Neg, a, Type::dimensionless_scalar());
        let result = super::collect_value_ref_members(&expr);
        assert!(
            result.iter().any(|m| m == "a"),
            "expected \"a\" in result; got {:?}",
            result
        );
    }

    /// collect_value_ref_members: ITEM-#3 regression guard — a ValueRef nested
    /// under OptionSome (a variant the old `_ => vec![]` arm silently dropped) is
    /// now collected because `CompiledExpr::walk` recurses into OptionSome.
    #[test]
    fn test_collect_value_ref_members_option_some_regression() {
        use reify_core::Type;
        use reify_ir::CompiledExpr;

        let a = CompiledExpr::value_ref(crate::vcid("E", "a"), Type::dimensionless_scalar());
        let expr = CompiledExpr::option_some(a, Type::dimensionless_scalar());
        let result = super::collect_value_ref_members(&expr);
        assert!(
            result.iter().any(|m| m == "a"),
            "expected \"a\" nested under OptionSome to be collected; got {:?}",
            result
        );
    }

    /// collect_value_ref_members: wrong-member guard — collecting member "a" must
    /// NOT report member "b" as present.  Locks in that the walk-backed superset
    /// collection (item-#3 fix) does not make `.iter().any(|m| m == required)` call
    /// sites falsely match a different member.
    #[test]
    fn test_collect_value_ref_members_no_false_positive() {
        use reify_core::Type;
        use reify_ir::CompiledExpr;

        let expr = CompiledExpr::value_ref(crate::vcid("E", "a"), Type::dimensionless_scalar());
        let result = super::collect_value_ref_members(&expr);
        assert!(
            !result.iter().any(|m| m == "b"),
            "did not expect \"b\" in result for a ValueRef with member \"a\"; got {:?}",
            result
        );
    }

    /// collect_value_ref_members: non-ValueRef leaves (Literal, OptionNone) yield
    /// an empty result — documents that only ValueRef nodes contribute members.
    #[test]
    fn test_collect_value_ref_members_empty_for_non_value_ref_leaves() {
        use reify_core::Type;
        use reify_ir::{CompiledExpr, Value};

        // A bare numeric literal has no ValueRef anywhere in its tree.
        let literal = CompiledExpr::literal(Value::Int(42), Type::Int);
        let result = super::collect_value_ref_members(&literal);
        assert!(
            result.is_empty(),
            "expected empty result for a Literal; got {:?}",
            result
        );

        // OptionNone likewise carries no ValueRef.
        let none = CompiledExpr::option_none(Type::dimensionless_scalar());
        let result_none = super::collect_value_ref_members(&none);
        assert!(
            result_none.is_empty(),
            "expected empty result for OptionNone; got {:?}",
            result_none
        );
    }

    // ─── missing_paths_under contract ─────────────────────────────────────

    /// Prefix for every temp dir these `missing_paths_under` tests create, so
    /// SIGKILL debris under `/tmp` stays attributable to this suite (see
    /// `temp_dirs::prefixed_tempdir`'s "Names stay attributable" section).
    const MISSING_PATHS_TEMPDIR_PREFIX: &str = "reify-missing-paths-under-";

    /// Materialise a "present" fixture at `dir.join(rel)`, creating any parent
    /// directories the forward-slash-separated `rel` implies.
    fn touch_under(dir: &std::path::Path, rel: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create parent dirs for fixture {rel:?}: {e}"));
        }
        std::fs::write(&path, "// present fixture\n")
            .unwrap_or_else(|e| panic!("write fixture {rel:?}: {e}"));
    }

    /// missing_paths_under: over a mixed skip list, exactly the entries with no
    /// file on disk come back — in input order.
    ///
    /// One case carries the whole contract on purpose. Entries are ordered
    /// missing, present, missing, present, so neither a short-circuit on the
    /// first miss nor an off-by-one can pass; the comparison is made WITHOUT
    /// sorting, so an order-scrambling implementation cannot pass either; and
    /// the keys are nested, forward-slash-separated paths — the real SKIP_SET
    /// key shape — so `dir.join(rel)` resolution is exercised for a present
    /// nested file, a missing sibling, and a path under an entirely absent
    /// subdirectory.
    #[test]
    fn test_missing_paths_under_flags_only_missing_entries_in_input_order() {
        let guard = crate::temp_dirs::prefixed_tempdir(MISSING_PATHS_TEMPDIR_PREFIX);
        let dir = guard.path();
        touch_under(dir, "topology_selectors/fillet_top_edges.ri");
        touch_under(dir, "present.ri");

        let missing = super::missing_paths_under(
            dir,
            [
                "topology_selectors/deleted_by_a_rename.ri",
                "topology_selectors/fillet_top_edges.ri",
                "auto/never_existed.ri",
                "present.ri",
            ],
        );

        assert_eq!(
            missing,
            vec![
                "topology_selectors/deleted_by_a_rename.ri",
                "auto/never_existed.ri"
            ],
            "expected exactly the two entries with no file on disk, in input order and \
             compared without sorting: an implementation that short-circuited on the first \
             miss would drop 'auto/never_existed.ri', and neither materialised fixture \
             (nested or top-level) may be flagged; got {missing:?}"
        );
    }

    /// missing_paths_under: an empty input iterator yields an empty `Vec`
    /// rather than panicking, even when `dir` names a path that does not
    /// exist. (Whether the call touches the filesystem at all is not something
    /// this test can observe, so it does not claim it.)
    #[test]
    fn test_missing_paths_under_empty_input_yields_empty_vec() {
        let empty: [&str; 0] = [];
        let missing =
            super::missing_paths_under(std::path::Path::new("/definitely/not/a/real/dir"), empty);

        assert!(
            missing.is_empty(),
            "expected an empty input iterator to yield an empty Vec, even for a `dir` that \
             does not exist; got {missing:?}"
        );
    }

    /// missing_paths_under: a dangling symlink reports as *missing*, pinning the
    /// documented `Path::exists` semantics — it follows symlinks, so a link whose
    /// target is gone is indistinguishable from an absent path.
    ///
    /// This is the one documented filesystem behaviour with a real failure mode
    /// behind it: an `examples/` entry that decays into a dangling link trips a
    /// SKIP_SET guard exactly as a deleted file would.
    #[cfg(unix)]
    #[test]
    fn test_missing_paths_under_reports_dangling_symlink_as_missing() {
        let guard = crate::temp_dirs::prefixed_tempdir(MISSING_PATHS_TEMPDIR_PREFIX);
        let dir = guard.path();
        touch_under(dir, "live_target.ri");
        std::os::unix::fs::symlink(dir.join("live_target.ri"), dir.join("live_link.ri"))
            .expect("create resolvable symlink fixture");
        std::os::unix::fs::symlink(dir.join("deleted_target.ri"), dir.join("dangling_link.ri"))
            .expect("create dangling symlink fixture");

        let missing = super::missing_paths_under(dir, ["live_link.ri", "dangling_link.ri"]);

        assert_eq!(
            missing,
            vec!["dangling_link.ri"],
            "expected the symlink whose target is gone to report as missing and the one \
             pointing at a live file not to — Path::exists resolves through the link; \
             got {missing:?}"
        );
    }
}
