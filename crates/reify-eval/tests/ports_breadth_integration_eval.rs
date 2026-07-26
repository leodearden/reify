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
//! EVAL-TIME RE-DECLARATION NOTE: compile_with_stdlib provides stdlib
//! definitions as a compilation prelude but only user templates appear in the
//! output CompiledModule. The eval engine resolves sub/structure constructions
//! exclusively from user templates, so every structure constructed at eval time
//! (ThreadSpec, Frame3) must be locally re-declared in the example .ri file.
//! This is the established m8_tolerancing.ri / ports_mechanical_thread_eval.rs
//! workaround (PRD §8 "Eval-time sub re-declaration").

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

/// Read `examples/stdlib/ports_breadth.ri`, parse, compile with stdlib
/// (asserting no Severity::Error at parse + compile), eval with
/// `SimpleConstraintChecker` (asserting no eval errors), and return the full
/// `EvalResult`. Mirrors `eval_ri_file` in m8_3_stdlib_integration.rs.
fn eval_breadth() -> reify_eval::EvalResult {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));

    // Prelude-aware parsing, so `Type.Variant` references against stdlib enums
    // (`ThreadSystem.ISO_Metric`, `FluidType.Liquid`, …) lower to `EnumAccess`
    // rather than `MemberAccess` — see `parse_with_stdlib`. Required now that
    // the example no longer re-declares those enums locally: a bare
    // `reify_syntax::parse` only disambiguates enums it can see in-file, so it
    // would report them as `unresolved name`. Matches the `compile_with_stdlib`
    // companion below and `examples_smoke.rs`.
    let parsed = reify_compiler::parse_with_stdlib(&source, ModulePath::single("ports_breadth"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in ports_breadth.ri: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
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
/// return the `CompiledModule` WITHOUT asserting on warnings — so warning
/// diagnostics are inspectable by the caller. Panics only on Severity::Error at
/// parse or compile stages.
fn compile_breadth() -> CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));

    // Prelude-aware parsing, so `Type.Variant` references against stdlib enums
    // (`ThreadSystem.ISO_Metric`, `FluidType.Liquid`, …) lower to `EnumAccess`
    // rather than `MemberAccess` — see `parse_with_stdlib`. Required now that
    // the example no longer re-declares those enums locally: a bare
    // `reify_syntax::parse` only disambiguates enums it can see in-file, so it
    // would report them as `unresolved name`. Matches the `compile_with_stdlib`
    // companion below and `examples_smoke.rs`.
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
/// standards-table values via the locally-re-declared ThreadSpec + ThreadAssembly
/// in ports_breadth.ri:
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

/// PRD docs/prds/v0_6/resolution-unification.md §7 boundary #18: once the stale
/// eval-time mirror is stripped from `examples/stdlib/ports_breadth.ri`, the
/// `ThreadSpec` constructed by `ThreadAssembly.spec` resolves against the
/// *stdlib* definition, not a local shadow — and therefore carries the stdlib
/// param the mirror had drifted away from.
///
/// The signal is `thread_form : Option<Geometry> = none`
/// (crates/reify-compiler/stdlib/ports_mechanical.ri:72), which the local
/// `structure def ThreadSpec` mirror at ports_breadth.ri:44-55 does not declare.
/// A local re-declaration silently shadows the stdlib prelude def, so while the
/// mirror is present no `ThreadAssembly.spec.thread_form` cell exists at all.
///
/// RED on base HEAD: `reify eval examples/stdlib/ports_breadth.ri` yields zero
/// occurrences of `thread_form`, so the cell lookup below fails.
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

/// Structural drift guard for resolution-unification α: the example must
/// re-declare NOTHING that stdlib already owns.
///
/// Asserts on the *compiled module* rather than the .ri source text, so the
/// guard pins compiler output (what actually shadows) and survives
/// reformatting or comment edits. `compile_with_stdlib` emits only the user's
/// own definitions — prelude defs are resolution context, not output (see
/// `compile_with_prelude_context` docs) — so anything appearing here was
/// declared by ports_breadth.ri itself.
///
/// RED after the ThreadSpec strip: `Frame3` is still a local template and all
/// five enums (ThreadSystem, ThreadClass, ThreadTighteningDirection, FluidType,
/// FittingStandard) are still local enum defs.
#[test]
fn example_declares_no_stdlib_shadowing_defs() {
    let compiled = compile_breadth();

    // Structures genuinely owned by this example — everything else is a mirror
    // of a stdlib type and must resolve through the prelude instead.
    const LOCAL_STRUCTURES: [&str; 3] = ["ThreadAssembly", "AsymmetricRig", "HydroConformer"];

    // Names stdlib owns that this example historically mirrored, mapped to the
    // stdlib file that owns them — so a failure names the real owner. Paths are
    // relative to crates/reify-compiler/stdlib/.
    const STDLIB_OWNER: [(&str, &str); 7] = [
        ("ThreadSpec", "ports_mechanical.ri"),
        ("ThreadSystem", "ports_mechanical.ri"),
        ("ThreadClass", "ports_mechanical.ri"),
        ("ThreadTighteningDirection", "ports_mechanical.ri"),
        ("Frame3", "ports.ri"),
        ("FluidType", "ports_fluid.ri"),
        ("FittingStandard", "ports_fluid.ri"),
    ];
    let owner_of = |name: &str| -> String {
        let file = STDLIB_OWNER
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, f)| *f)
            .unwrap_or("<some std.ports submodule>");
        format!("crates/reify-compiler/stdlib/{}", file)
    };

    // (a) The only Structure templates are the three genuinely-local ones.
    let mut structures: Vec<&str> = compiled
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    structures.sort_unstable();

    let shadowed: Vec<&str> = structures
        .iter()
        .copied()
        .filter(|n| !LOCAL_STRUCTURES.contains(n))
        .collect();
    assert!(
        shadowed.is_empty(),
        "examples/stdlib/ports_breadth.ri re-declares structure(s) stdlib already owns: {}. \
         Delete the local mirror and let it resolve through the stdlib prelude \
         (resolution-unification.md §8 α / boundary #18).",
        shadowed
            .iter()
            .map(|n| format!("`{}` (owned by {})", n, owner_of(n)))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut expected = LOCAL_STRUCTURES.to_vec();
    expected.sort_unstable();
    assert_eq!(
        structures, expected,
        "ports_breadth.ri should declare exactly its three own structures; \
         a missing one means the example lost a signal carrier"
    );

    // (b) No user-declared enums at all — every enum it uses is stdlib's.
    let local_enums: Vec<&str> = compiled.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert!(
        local_enums.is_empty(),
        "examples/stdlib/ports_breadth.ri re-declares enum(s) stdlib already owns: {}. \
         Delete the local mirrors — stdlib enum variants resolve through the prelude \
         at both compile and eval (resolution-unification.md §8 α / boundary #18).",
        local_enums
            .iter()
            .map(|n| format!("`{}` (owned by {})", n, owner_of(n)))
            .collect::<Vec<_>>()
            .join(", ")
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
