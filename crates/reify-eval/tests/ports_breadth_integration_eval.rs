//! Task η integration-gate: multi-domain example + eval/diagnostic assertions
//! (PRD docs/prds/v0_6/ports-breadth-expansion.md §7 η).
//!
//! Exercises the shared example file `examples/stdlib/ports_breadth.ri` through
//! the full parse → compile_with_stdlib → eval pipeline and asserts three η
//! signals in TDD order:
//!
//!   a. `thread_spec_derived_lets_eval_from_example` — ThreadSpec ISO 68-1
//!      derived lets (M6×1) resolve to standards-table values.
//!   b. `asymmetric_located_port_warning_fires` — a MechanicalPort (located) ↔
//!      ThermalPort (non-located) bare connect emits the asymmetric-LocatedPort
//!      warning (connect.rs:344).
//!   c. `hydraulic_port_multidomain_conforms` — HydroConformer's port p
//!      type_name == "HydraulicPort" (FluidPort+MechanicalPort diamond resolves,
//!      zero Error diagnostics).
//!
//! and two resolution-unification α signals (boundary #18):
//!
//!   d. `stdlib_thread_spec_thread_form_resolves_at_eval` — ThreadAssembly.spec
//!      carries the stdlib-only `thread_form` param, proving the construction
//!      resolves against stdlib rather than a local shadow.
//!   e. `example_declares_exactly_its_own_defs` — structural drift guard: the
//!      example's compiled templates are exactly its own three, and it declares
//!      no enum, trait, or function at all — every one of those it names is
//!      stdlib's, resolved through the prelude.
//!
//! The example .ri file re-declares nothing: stdlib defs resolve through the
//! prelude fallback at eval, so the mirrors it used to carry are gone
//! (docs/prds/v0_6/resolution-unification.md §8 α, boundary #18).

use reify_compiler::{CompiledModule, EntityKind};
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::make_simple_engine;

// ── File path ─────────────────────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/stdlib/ports_breadth.ri"
);

// ── Helpers ───────────────────────────────────────────────────────────────────

/// `compile_breadth()` plus a compile-stage Severity::Error assertion, then eval
/// with `SimpleConstraintChecker` (asserting no eval errors), returning the full
/// `EvalResult`. Mirrors `eval_ri_file` in m8_3_stdlib_integration.rs.
fn eval_breadth() -> reify_eval::EvalResult {
    let compiled = compile_breadth();

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors in ports_breadth.ri: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in ports_breadth.ri: {:?}",
        eval_errors
    );

    result
}

/// Read `examples/stdlib/ports_breadth.ri`, parse, compile with stdlib, and
/// return the `CompiledModule` WITHOUT asserting on diagnostics — so both
/// warnings and errors are inspectable by the caller. Panics only on a parse
/// error. `eval_breadth()` layers the compile-stage Error assertion on top.
fn compile_breadth() -> CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));

    // Prelude-aware parsing, so `Type.Variant` references against stdlib enums
    // (`ThreadSystem.ISO_Metric`, `FluidType.Liquid`, …) lower to `EnumAccess`
    // rather than `MemberAccess` — see `parse_with_stdlib`. Required now that
    // the example no longer re-declares those enums locally: a bare
    // `reify_syntax::parse` only disambiguates enums it can see in-file, so it
    // would report them as `unresolved name`. Matches `examples_smoke.rs`.
    let parsed = reify_compiler::parse_with_stdlib(&source, ModulePath::single("ports_breadth"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in ports_breadth.ri: {:?}",
        parsed.errors
    );

    reify_compiler::compile_with_stdlib(&parsed)
}

/// Fetch a Scalar cell from the eval result and assert its si_value (abs tol
/// 1e-9) and dimension. Mirrors `assert_scalar_cell` in
/// m8_3_stdlib_integration.rs / ports_mechanical_thread_eval.rs.
fn assert_scalar_cell(
    result: &reify_eval::EvalResult,
    entity: &str,
    member: &str,
    expected_si: f64,
    expected_dim: DimensionVector,
) {
    let id = ValueCellId::new(entity, member);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{}.{} not found in eval result", entity, member));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - expected_si).abs() < 1e-9,
                "{}.{}: expected si_value ≈{}, got {}",
                entity,
                member,
                expected_si,
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "{}.{}: wrong dimension",
                entity, member
            );
        }
        other => panic!(
            "{}.{} should be Value::Scalar, got {:?}",
            entity, member, other
        ),
    }
}

// ─── step-1/2 (task η signal a): ThreadSpec derived-let eval readout ─────────

/// PRD task-η signal (a): ThreadSpec derived lets (M6×1) resolve to ISO 68-1
/// standards-table values. The lets come from the stdlib ThreadSpec
/// (crates/reify-compiler/stdlib/ports_mechanical.ri), constructed by
/// ThreadAssembly in ports_breadth.ri:
///   minor_diameter = 6 − 1·1.0825 = 4.9175mm = 0.0049175m
///   pitch_diameter = 6 − 1·0.6495 = 5.3505mm = 0.0053505m
///   tap_drill      = 6 − 1        = 5mm      = 0.005m
///
/// RED (step-1): ports_breadth.ri does not exist → read_to_string panics.
#[test]
fn thread_spec_derived_lets_eval_from_example() {
    let result = eval_breadth();

    assert_scalar_cell(
        &result,
        "ThreadAssembly.spec",
        "minor_diameter",
        0.0049175,
        DimensionVector::LENGTH,
    );
    assert_scalar_cell(
        &result,
        "ThreadAssembly.spec",
        "pitch_diameter",
        0.0053505,
        DimensionVector::LENGTH,
    );
    assert_scalar_cell(
        &result,
        "ThreadAssembly.spec",
        "tap_drill",
        0.005,
        DimensionVector::LENGTH,
    );
}

// ─── resolution-unification α (boundary #18): stdlib ThreadSpec resolves ──────

/// PRD docs/prds/v0_6/resolution-unification.md §7 boundary #18: with the stale
/// eval-time mirror stripped from `examples/stdlib/ports_breadth.ri`, the
/// `ThreadSpec` constructed by `ThreadAssembly.spec` resolves against the
/// *stdlib* definition, not a local shadow — and therefore carries the stdlib
/// param the mirror had drifted away from.
///
/// The signal is `thread_form : Option<Geometry> = none`
/// (crates/reify-compiler/stdlib/ports_mechanical.ri:72). The local
/// `structure def ThreadSpec` mirror removed in this change (task 5515) omitted
/// it, and a local re-declaration silently shadows the stdlib prelude def — so
/// while that mirror was present no `ThreadAssembly.spec.thread_form` cell
/// existed at all. A re-introduced mirror would make it disappear again.
///
/// RED on base HEAD (mirror still present): `reify eval
/// examples/stdlib/ports_breadth.ri` yielded zero occurrences of `thread_form`,
/// so the cell lookup below failed.
///
/// This test doubles as a durable mirror-reintroduction guard: if someone
/// re-adds a local `ThreadSpec`, the cell disappears again and this goes red
/// with a readout of the members that *were* produced.
#[test]
fn stdlib_thread_spec_thread_form_resolves_at_eval() {
    let result = eval_breadth();

    let id = ValueCellId::new("ThreadAssembly.spec", "thread_form");
    let val = result.values.get(&id).unwrap_or_else(|| {
        let mut members: Vec<&str> = result
            .values
            .iter()
            .filter(|(k, _)| k.entity == "ThreadAssembly.spec")
            .map(|(k, _)| k.member.as_str())
            .collect();
        members.sort_unstable();
        panic!(
            "ThreadAssembly.spec.thread_form not found in eval result — the stdlib \
             ThreadSpec (crates/reify-compiler/stdlib/ports_mechanical.ri:72, \
             `param thread_form : Option<Geometry> = none`) did not reach eval. \
             Most likely a local `structure def ThreadSpec` mirror was reintroduced \
             into examples/stdlib/ports_breadth.ri and is shadowing it. \
             ThreadAssembly.spec members present: {:?}",
            members
        )
    });

    assert!(
        matches!(val, Value::Option(None)),
        "ThreadAssembly.spec.thread_form should be Value::Option(None) (the stdlib \
         default `= none`), got {:?}",
        val
    );
}

/// Structural drift guard for resolution-unification α: the example declares
/// exactly its own three templates, and no enum, trait, or function at all.
///
/// Asserts on the *compiled module* rather than the .ri source text, so the
/// guard pins compiler output (what actually shadows) and survives
/// reformatting or comment edits. `compile_with_stdlib` emits only the user's
/// own definitions — prelude defs are resolution context, not output (see
/// `compile_with_prelude_context` docs) — so anything appearing here was
/// declared by ports_breadth.ri itself.
///
/// Every declaration form a reintroduced mirror could take is pinned, not just
/// the two forms the α strip happened to remove:
///   - templates across ALL `EntityKind`s (`Structure` and `Occurrence`), since
///     `occurrence def ThreadSpec` shadows exactly as well as `structure def
///     ThreadSpec` — verified: an added `occurrence def` fails this test;
///   - enum defs — the five stripped thread/fluid enums;
///   - trait defs — the quietest failure mode, because a local
///     `trait MechanicalPort` / `FluidPort` changes which port definition
///     signals (b)/(c) above resolve against without changing any name they
///     assert on;
///   - functions, so the "declares nothing else" claim holds for every form.
///
/// RED after the ThreadSpec strip: `Frame3` was still a local template and all
/// five enums (ThreadSystem, ThreadClass, ThreadTighteningDirection, FluidType,
/// FittingStandard) were still local enum defs.
#[test]
fn example_declares_exactly_its_own_defs() {
    let compiled = compile_breadth();

    // Templates genuinely owned by this example. Every other name it uses
    // belongs to stdlib and must resolve through the prelude instead.
    const LOCAL_TEMPLATES: [&str; 3] = ["ThreadAssembly", "AsymmetricRig", "HydroConformer"];

    let prd = "docs/prds/v0_6/resolution-unification.md §8 α / boundary #18";

    // (a) Templates, every EntityKind. One assertion covers both directions: an
    // extra name is a reintroduced mirror (or a new local def), a missing one is
    // a signal carrier the η tests above depend on.
    let mut templates: Vec<&str> = compiled.templates.iter().map(|t| t.name.as_str()).collect();
    templates.sort_unstable();
    let mut expected = LOCAL_TEMPLATES.to_vec();
    expected.sort_unstable();
    assert_eq!(
        templates, expected,
        "examples/stdlib/ports_breadth.ri should declare exactly its own templates. \
         An extra name that stdlib owns is a stale mirror — delete it and let the prelude \
         resolve it; an extra name stdlib does not own means the example grew a local def, \
         so widen LOCAL_TEMPLATES. A missing name means the example lost an η signal \
         carrier. ({prd})"
    );

    // (b) No enum defs at all: ThreadSystem / ThreadClass /
    // ThreadTighteningDirection are stdlib ports_mechanical.ri's, FluidType /
    // FittingStandard are ports_fluid.ri's. Unlike (a) there is no allowlist, so
    // the remediation for a legitimately-local one differs — say so explicitly.
    let local_enums: Vec<&str> = compiled.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert!(
        local_enums.is_empty(),
        "examples/stdlib/ports_breadth.ri should declare no enums; got {local_enums:?}. \
         Every enum it uses is stdlib's and resolves through the prelude at both compile \
         and eval, so a local copy is a stale mirror — delete it. If the example ever \
         legitimately needs an enum of its own, this assertion has no allowlist to widen: \
         replace it with an expected-set assert_eq! shaped like (a) above. ({prd})"
    );

    // (c) No trait defs: the port traits (MechanicalPort, ThermalPort, FluidPort,
    // HydraulicPort, LocatedPort) are stdlib's. Same no-allowlist remediation as (b).
    let local_traits: Vec<&str> = compiled
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        local_traits.is_empty(),
        "examples/stdlib/ports_breadth.ri should declare no traits; got {local_traits:?}. \
         The port traits are stdlib's; a local re-declaration keeps every name signals \
         (b)/(c) assert on while silently changing which definition they resolve against — \
         delete it. A legitimately-local trait needs this assertion replaced by an \
         expected-set assert_eq! shaped like (a) above. ({prd})"
    );

    // (d) No functions: the only one the example calls is `vec3`, a compiler
    // builtin (MATH_CONSTRUCTION_NAMES in reify-compiler/src/math_signatures.rs),
    // not a stdlib `fn`. Same no-allowlist remediation as (b).
    let local_fns: Vec<&str> = compiled.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(
        local_fns.is_empty(),
        "examples/stdlib/ports_breadth.ri should declare no functions; got {local_fns:?}. \
         A local definition shadowing a name the example already resolves elsewhere (the \
         builtin `vec3`, or any stdlib `fn`) would change the numerics the η signals assert \
         on — delete it. A legitimately-local function needs this assertion replaced by an \
         expected-set assert_eq! shaped like (a) above. ({prd})"
    );
}

// ─── step-3/4 (task η signal b): asymmetric LocatedPort warning ───────────────

/// PRD task-η signal (b): connecting a stdlib MechanicalPort (LocatedPort) to a
/// ThermalPort (non-located) via a bare port name emits the asymmetric-LocatedPort
/// warning (connect.rs:344).
///
/// Mirrors connect_compile_tests.rs::asymmetric_located_port_emits_warning but
/// exercises stdlib-derived trait-typed ports rather than inline ad-hoc traits —
/// the integration point the PRD §0 highlights ("previously unreachable from
/// stdlib-derived ports").
///
/// RED (step-3): ports_breadth.ri has no asymmetric connect yet → no warning.
#[test]
fn asymmetric_located_port_warning_fires() {
    let compiled = compile_breadth();

    let located_warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("asymmetric")
                && d.message.contains("LocatedPort")
        })
        .collect();

    assert!(
        !located_warnings.is_empty(),
        "expected a warning about asymmetric LocatedPort connection in ports_breadth.ri \
         (MechanicalPort [located] ↔ ThermalPort [non-located]), got diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ─── step-5/6 (task η signal c): HydraulicPort multi-domain conformance ───────

/// PRD task-η signal (c): HydroConformer's port p resolves to type_name
/// "HydraulicPort" (FluidPort + MechanicalPort multi-domain diamond), and the
/// whole file compiles with zero Severity::Error diagnostics.
///
/// Mirrors ports_stdlib_compile.rs::hydraulic_port_concrete_conformer_multidomain_compiles
/// + the m8_3 ports port type_name assertion idiom.
///
/// RED (step-5): HydroConformer does not exist in ports_breadth.ri yet →
/// the find on compiled.templates panics / asserts.
#[test]
fn hydraulic_port_multidomain_conforms() {
    let compiled = compile_breadth();

    // (i) Zero Error diagnostics for the whole file.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "ports_breadth.ri should have zero Error diagnostics; got: {:?}",
        errors
    );

    // (ii) HydroConformer is declared as a Structure template.
    let hydro = compiled
        .templates
        .iter()
        .find(|t| t.name == "HydroConformer" && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "HydroConformer (EntityKind::Structure) not found in ports_breadth.ri; \
                 templates: {:?}",
                compiled
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        });

    // (iii) Port p type_name == "HydraulicPort".
    let port_p = hydro
        .ports
        .iter()
        .find(|p| p.name == "p")
        .expect("HydroConformer should have a port named 'p'");
    assert_eq!(
        port_p.type_name, "HydraulicPort",
        "HydroConformer.p port type_name should be 'HydraulicPort' (FluidPort+MechanicalPort \
         multi-domain diamond), got '{}'",
        port_p.type_name
    );
}
