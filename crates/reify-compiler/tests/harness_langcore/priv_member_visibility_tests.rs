//! Compiled-IR member-visibility wiring + enforcement tests (task #3978 δ —
//! `module-and-visibility-hardening.md` Slice C, steps 3–6).
//!
//! ## Part A — AST `is_priv` → compiled visibility contract (steps 3/4)
//!
//! Pins the invariant that `priv param`, `priv sub`, and `priv port` lower into
//! the compiled IR with the correct private-visibility markers:
//!
//! - `priv param p` → `ValueCellDecl.visibility == Visibility::Private`
//! - `param q`      → `ValueCellDecl.visibility == Visibility::Public`
//! - `priv sub a`   → `SubComponentDecl.visibility == Visibility::Private`
//! - `sub b`        → `SubComponentDecl.visibility == Visibility::Public`
//! - `priv port pt` → `CompiledPort.is_priv == true`   (field added in step-4)
//! - `port pu`      → `CompiledPort.is_priv == false`
//!
//! Step-3 RED: params/subs are hardcoded `Visibility::Public`, and `CompiledPort`
//! has no `is_priv` field (a hard compile error). Turns GREEN after step-4 wires
//! `is_priv` at the `entity.rs` construction sites + adds the `CompiledPort` field.
//!
//! ## Part B — E_PRIV_MEMBER_ACCESS single-module enforcement (steps 5/6)
//!
//! Appended in step-5: external dot-access on a `priv` member emits
//! `E_PRIV_MEMBER_ACCESS`, while internal (self-body) access and non-priv member
//! access stay clean. RED until step-6 wires the `expr.rs` enforcement.

use std::sync::OnceLock;

use reify_compiler::{ValueCellKind, Visibility};
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source;

// ── Source fixture ───────────────────────────────────────────────────────────

/// `Motor` exercises all three priv / non-priv member pairs (param, sub, port).
fn motor_source() -> &'static str {
    r#"
trait SomeTrait {}

structure def Inner {}

structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
    priv sub a = Inner()
    sub b = Inner()
    priv port pt : SomeTrait {}
    port pu : SomeTrait {}
}
"#
}

/// Locate the `Motor` template in the compiled module.
fn motor_template(module: &reify_compiler::CompiledModule) -> &reify_compiler::TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == "Motor")
        .expect("Motor template not found in compiled module")
}

// ── Part A: AST is_priv → compiled visibility ─────────────────────────────────

/// `priv param p` must lower to `Visibility::Private`.
#[test]
fn priv_param_compiles_to_visibility_private() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let p_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "p" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'p' (Param kind) not found in Motor template");

    assert_eq!(
        p_cell.visibility,
        Visibility::Private,
        "priv param p must compile to Visibility::Private, got {:?}",
        p_cell.visibility
    );
}

/// Plain `param q` must stay `Visibility::Public` (no regression).
#[test]
fn plain_param_compiles_to_visibility_public() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let q_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "q" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'q' (Param kind) not found in Motor template");

    assert_eq!(
        q_cell.visibility,
        Visibility::Public,
        "plain param q must compile to Visibility::Public, got {:?}",
        q_cell.visibility
    );
}

/// `priv sub a = Inner()` must lower to `SubComponentDecl.visibility == Private`.
#[test]
fn priv_sub_compiles_to_visibility_private() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let sub_a = template
        .sub_components
        .iter()
        .find(|s| s.name == "a")
        .expect("sub_component 'a' not found in Motor template");

    assert_eq!(
        sub_a.visibility,
        Visibility::Private,
        "priv sub a must compile to Visibility::Private, got {:?}",
        sub_a.visibility
    );
}

/// Plain `sub b = Inner()` must stay `Visibility::Public` (no regression).
#[test]
fn plain_sub_compiles_to_visibility_public() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let sub_b = template
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("sub_component 'b' not found in Motor template");

    assert_eq!(
        sub_b.visibility,
        Visibility::Public,
        "plain sub b must compile to Visibility::Public, got {:?}",
        sub_b.visibility
    );
}

/// `priv port pt` must lower to `CompiledPort.is_priv == true`.
#[test]
fn priv_port_compiles_to_is_priv_true() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let port_pt = template
        .ports
        .iter()
        .find(|p| p.name == "pt")
        .expect("port 'pt' not found in Motor template");

    assert!(
        port_pt.is_priv,
        "priv port pt must compile to is_priv == true"
    );
}

/// Plain `port pu` must stay `is_priv == false` (no regression).
#[test]
fn plain_port_compiles_to_is_priv_false() {
    let module = compile_source(motor_source());
    let template = motor_template(&module);

    let port_pu = template
        .ports
        .iter()
        .find(|p| p.name == "pu")
        .expect("port 'pu' not found in Motor template");

    assert!(
        !port_pu.is_priv,
        "plain port pu must compile to is_priv == false"
    );
}

// ── Part B: E_PRIV_MEMBER_ACCESS single-module enforcement (steps 5/6) ─────────

/// Collect the `E_PRIV_MEMBER_ACCESS` errors emitted while compiling `module`.
fn priv_access_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::PrivMemberAccess))
        .collect()
}

/// External dot-access on a `priv param` emits exactly one E_PRIV_MEMBER_ACCESS.
#[test]
fn external_priv_param_access_emits_error() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
}

structure def Parent {
    sub m = Motor()
    let touch = m.p
}
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "external access to `m.p` (priv param) must emit exactly one E_PRIV_MEMBER_ACCESS; \
         all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
    assert!(
        priv_errs[0].message.contains('p'),
        "diagnostic should name the offending member: {}",
        priv_errs[0].message
    );
}

/// External dot-access on a default-visible `param` resolves with no priv error.
#[test]
fn external_pub_param_access_ok() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
}

structure def Parent {
    sub m = Motor()
    let touch = m.q
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `m.q` (default-visible param) must NOT emit E_PRIV_MEMBER_ACCESS; \
         all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// External dot-access on a `priv sub` emits E_PRIV_MEMBER_ACCESS.
#[test]
fn external_priv_sub_access_emits_error() {
    let module = compile_source(
        r#"
structure def Inner {}

structure def Holder {
    priv sub a = Inner()
    sub b = Inner()
}

structure def Parent {
    sub h = Holder()
    let touch = h.a
}
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "external access to `h.a` (priv sub) must emit exactly one E_PRIV_MEMBER_ACCESS; \
         all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
}

/// External dot-access on a default-visible `sub` resolves with no priv error.
#[test]
fn external_pub_sub_access_ok() {
    let module = compile_source(
        r#"
structure def Inner {}

structure def Holder {
    priv sub a = Inner()
    sub b = Inner()
}

structure def Parent {
    sub h = Holder()
    let touch = h.b
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `h.b` (default-visible sub) must NOT emit E_PRIV_MEMBER_ACCESS; \
         all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// External dot-access on a `priv port` emits E_PRIV_MEMBER_ACCESS.
#[test]
fn external_priv_port_access_emits_error() {
    let module = compile_source(
        r#"
trait SomeTrait {}

structure def PortHolder {
    priv port pt : SomeTrait {}
    port pu : SomeTrait {}
}

structure def Parent {
    sub ph = PortHolder()
    let touch = ph.pt
}
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "external access to `ph.pt` (priv port) must emit exactly one E_PRIV_MEMBER_ACCESS; \
         all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
}

/// A `priv param` referenced from INSIDE its own structure body — by bare name
/// and via `self.p` — is exempt (internal access stays free; only external
/// `obj.member` dot-access is gated).
#[test]
fn internal_priv_param_access_ok() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
    let internal = p
    constraint self.p >= 0
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "internal references to a priv param (bare `p` and `self.p`) must NOT emit \
         E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Part C: other external-access paths — function body + purpose subject ─────
//
// A priv member must stay hidden on EVERY external access path, not just the
// in-structure dot-access branch. These guard the two leaks found in review:
// function bodies (the skeleton template) and purpose subjects.

/// A function body reading a structure-typed param's `priv` member is external
/// access → E_PRIV_MEMBER_ACCESS.
#[test]
fn function_body_priv_member_access_emits_error() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
}

fn leak(m : Motor) -> Real { m.p }
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "function-body access to `m.p` (priv param) must emit exactly one E_PRIV_MEMBER_ACCESS; \
         all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
}

/// A function body reading a default-visible member resolves with no priv error.
#[test]
fn function_body_pub_member_access_ok() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
}

fn ok(m : Motor) -> Real { m.q }
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "function-body access to `m.q` (default-visible param) must NOT emit \
         E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// A purpose subject reading a `priv` member is external access → E_PRIV_MEMBER_ACCESS.
#[test]
fn purpose_subject_priv_member_access_emits_error() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
}

purpose checkit(subject : Motor) {
    constraint subject.p > 0
}
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "purpose-subject access to `subject.p` (priv param) must emit exactly one \
         E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
}

/// A purpose subject reading a default-visible member resolves with no priv error.
#[test]
fn purpose_subject_pub_member_access_ok() {
    let module = compile_source(
        r#"
structure def Motor {
    priv param p : Real = 0
    param q : Real = 0
}

purpose okp(subject : Motor) {
    constraint subject.q > 0
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "purpose-subject access to `subject.q` (default-visible param) must NOT emit \
         E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Part D: priv param inside port-member and guarded-block visibility ────────
//
// Task #5161: `priv param x` inside a `port { }` block or a block-form
// `where cond { }` guarded block was silently lowered to `Visibility::Public`
// at four hardcoded ValueCellDecl construction sites — entity.rs's port-member
// arm and guards.rs's `compile_guarded_members` (each has an `Auto { free }`
// and a `Param` branch). This fixes the lowered `ValueCellDecl.visibility`
// field to match `param.is_priv` at all four sites.
//
// Scope note: `template_member_is_priv` (expr.rs) — the sole predicate behind
// E_PRIV_MEMBER_ACCESS enforcement — didn't scan `ports[].members[].visibility`
// or `guarded_groups[].members[].visibility`, so the field fixed here wasn't
// yet consulted for these two member kinds; the priv gate couldn't fire for
// them. Enforcing the now-correct visibility field was out of scope for
// #5161 and tracked as follow-up #5171 ("Enforce lowered priv visibility in
// E_PRIV_MEMBER_ACCESS (port-member / guarded-block params)") — which has
// now landed for EXTERNAL access: guarded-block members via an extended
// `template_member_is_priv` plus reordering the priv check ahead of the
// StructureMemberNotFound early-return, and port members (the two-level
// `<sub>.<port>.<member>` shape) via a dedicated AST-pattern branch in
// `compile_member_access` (see the `_emits_error` tests below). Function-body
// access is now enforced too: `build_structure_def_skeleton` populates the
// skeleton's `ports`/`guarded_groups` with the nested `priv`-aware members, and
// the port case is completed by an `expr.rs` branch that resolves a fn-body
// `StructureRef` receiver (see the `..._priv_gated` tests further below). Part D
// itself asserts only the lowered `visibility` field, not dot-access
// enforcement, so it stays green independent of that enforcement.

/// Locate a template by name in the compiled module (generalizes `motor_template`
/// for the Part D fixtures below, which use distinct structure names).
fn find_template<'a>(
    module: &'a reify_compiler::CompiledModule,
    name: &str,
) -> &'a reify_compiler::TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{name} template not found in compiled module"))
}

/// Assert `module` compiled its fixture cleanly (no error-severity
/// diagnostics). Guards the Part D field-only assertions below against a
/// false pass: without this, a fixture that regressed to a partial compile
/// (e.g. an unrelated diagnostic on `priv param aux = auto(free)`) could
/// still surface a findable, correctly-`Private` value cell and mask the
/// regression.
fn assert_fixture_compiles_cleanly(module: &reify_compiler::CompiledModule, fixture_name: &str) {
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{fixture_name} fixture must compile without error-severity diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `PortHost` exercises `priv param` inside a `port { }` block, covering both
/// value-cell branches (plain Param and auto-form), plus a non-priv sibling
/// port to guard against a naive Visibility::Private hardcode.
fn port_host_source() -> &'static str {
    r#"
trait Iface {}

structure def PortHost {
    port secret : Iface {
        priv param main : Length = 5mm
        priv param aux : Length = auto(free)
    }
    port plain : Iface {
        param vis : Length = 5mm
    }
}
"#
}

/// Cached compile of [`port_host_source`], shared by the three `PortHost`
/// tests below. The fixture is deterministic, so compiling it once (mirroring
/// the `OnceLock`-backed caching idiom in
/// `stdlib_loader::load_stdlib`/`units_module`) avoids three redundant full
/// compiles for no isolation benefit. A broken fixture panics here (via
/// `assert_fixture_compiles_cleanly`) rather than being cached silently
/// broken.
fn port_host_module() -> &'static reify_compiler::CompiledModule {
    static CACHE: OnceLock<reify_compiler::CompiledModule> = OnceLock::new();
    CACHE.get_or_init(|| {
        let module = compile_source(port_host_source());
        assert_fixture_compiles_cleanly(&module, "PortHost");
        module
    })
}

/// `priv param main` inside `port secret { }` (Param-kind value cell) must
/// lower to `Visibility::Private`.
#[test]
fn priv_param_in_port_compiles_to_visibility_private() {
    let module = port_host_module();
    let template = find_template(module, "PortHost");
    let members: Vec<_> = template.ports.iter().flat_map(|p| &p.members).collect();

    let main_cell = members
        .iter()
        .find(|vc| vc.id.member == "secret.main" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'secret.main' (Param kind) not found in PortHost template");

    assert_eq!(
        main_cell.visibility,
        Visibility::Private,
        "priv param main inside port secret must compile to Visibility::Private, got {:?}",
        main_cell.visibility
    );
}

/// `priv param aux = auto(free)` inside `port secret { }` (Auto-kind value
/// cell) must lower to `Visibility::Private`.
#[test]
fn priv_auto_param_in_port_compiles_to_visibility_private() {
    let module = port_host_module();
    let template = find_template(module, "PortHost");
    let members: Vec<_> = template.ports.iter().flat_map(|p| &p.members).collect();

    let aux_cell = members
        .iter()
        .find(|vc| vc.id.member == "secret.aux" && matches!(vc.kind, ValueCellKind::Auto { .. }))
        .expect("value cell 'secret.aux' (Auto kind) not found in PortHost template");

    assert_eq!(
        aux_cell.visibility,
        Visibility::Private,
        "priv param aux = auto(free) inside port secret must compile to Visibility::Private, got {:?}",
        aux_cell.visibility
    );
}

/// Plain `param vis` inside `port plain { }` must stay `Visibility::Public`
/// (no regression / guards against a naive Visibility::Private hardcode).
#[test]
fn plain_param_in_port_compiles_to_visibility_public() {
    let module = port_host_module();
    let template = find_template(module, "PortHost");
    let members: Vec<_> = template.ports.iter().flat_map(|p| &p.members).collect();

    let vis_cell = members
        .iter()
        .find(|vc| vc.id.member == "plain.vis" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'plain.vis' (Param kind) not found in PortHost template");

    assert_eq!(
        vis_cell.visibility,
        Visibility::Public,
        "plain param vis inside port plain must compile to Visibility::Public, got {:?}",
        vis_cell.visibility
    );
}

/// `GuardHost` exercises `priv param` inside a BLOCK-form `where cond { }`
/// guarded block, covering both value-cell branches (plain Param and
/// auto-form), plus a non-priv sibling member to guard against a naive
/// Visibility::Private hardcode. Uses the block form (not the per-decl
/// `param x where cond`, which already routes through the correct
/// entity.rs structure-body site via compile_per_decl_guard).
fn guard_host_source() -> &'static str {
    r#"
structure def GuardHost {
    param active : Bool = true
    where active {
        priv param g : Length = 5mm
        priv param h : Length = auto
        param vis : Length = 5mm
    }
}
"#
}

/// Cached compile of [`guard_host_source`], shared by the three `GuardHost`
/// tests below. Same rationale as [`port_host_module`]: the fixture is
/// deterministic, so one compile (via `OnceLock`) replaces three redundant
/// ones. A broken fixture panics here (via `assert_fixture_compiles_cleanly`)
/// rather than being cached silently broken.
fn guard_host_module() -> &'static reify_compiler::CompiledModule {
    static CACHE: OnceLock<reify_compiler::CompiledModule> = OnceLock::new();
    CACHE.get_or_init(|| {
        let module = compile_source(guard_host_source());
        assert_fixture_compiles_cleanly(&module, "GuardHost");
        module
    })
}

/// `priv param g` inside a block-form `where active { }` (Param-kind value
/// cell) must lower to `Visibility::Private`.
#[test]
fn priv_param_in_guarded_block_compiles_to_visibility_private() {
    let module = guard_host_module();
    let template = find_template(module, "GuardHost");

    let g_cell = template.guarded_groups[0]
        .members
        .iter()
        .find(|vc| vc.id.member == "g" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'g' (Param kind) not found in GuardHost guarded_groups[0]");

    assert_eq!(
        g_cell.visibility,
        Visibility::Private,
        "priv param g inside where-block must compile to Visibility::Private, got {:?}",
        g_cell.visibility
    );
}

/// `priv param h = auto` inside a block-form `where active { }` (Auto-kind
/// value cell) must lower to `Visibility::Private`.
#[test]
fn priv_auto_param_in_guarded_block_compiles_to_visibility_private() {
    let module = guard_host_module();
    let template = find_template(module, "GuardHost");

    let h_cell = template.guarded_groups[0]
        .members
        .iter()
        .find(|vc| vc.id.member == "h" && matches!(vc.kind, ValueCellKind::Auto { .. }))
        .expect("value cell 'h' (Auto kind) not found in GuardHost guarded_groups[0]");

    assert_eq!(
        h_cell.visibility,
        Visibility::Private,
        "priv param h = auto inside where-block must compile to Visibility::Private, got {:?}",
        h_cell.visibility
    );
}

/// Plain `param vis` inside a block-form `where active { }` must stay
/// `Visibility::Public` (no regression / guards against a naive
/// Visibility::Private hardcode).
#[test]
fn plain_param_in_guarded_block_compiles_to_visibility_public() {
    let module = guard_host_module();
    let template = find_template(module, "GuardHost");

    let vis_cell = template.guarded_groups[0]
        .members
        .iter()
        .find(|vc| vc.id.member == "vis" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'vis' (Param kind) not found in GuardHost guarded_groups[0]");

    assert_eq!(
        vis_cell.visibility,
        Visibility::Public,
        "plain param vis inside where-block must compile to Visibility::Public, got {:?}",
        vis_cell.visibility
    );
}

/// `priv param g : Length = 5mm where active` — the per-declaration `where`
/// clause form — must also lower to `Visibility::Private`. Unlike the
/// block-form fixtures above (which exercise the guards.rs site this task
/// fixes), the per-decl form routes through `compile_per_decl_guard`, which
/// reuses the `ValueCellDecl` already built with the correct visibility at
/// the structure-body site (entity.rs's top-level `MemberDecl::Param` arm) —
/// the scope note above claims this path was "already correct" and thus out
/// of this task's fix scope, but that claim was previously untested here.
/// This characterization test regression-guards it: if a future change to
/// `compile_per_decl_guard` (or the structure-body param site upstream of
/// it) silently dropped the visibility through, this would catch it.
#[test]
fn priv_param_with_per_decl_where_compiles_to_visibility_private() {
    let module = compile_source(
        r#"
structure def PerDeclGuardHost {
    param active : Bool = true
    priv param g : Length = 5mm where active
}
"#,
    );
    assert_fixture_compiles_cleanly(&module, "PerDeclGuardHost");
    let template = find_template(&module, "PerDeclGuardHost");

    let g_cell = template
        .guarded_groups
        .iter()
        .flat_map(|g| &g.members)
        .find(|vc| vc.id.member == "g" && vc.kind == ValueCellKind::Param)
        .expect("value cell 'g' (Param kind) not found in PerDeclGuardHost guarded_groups");

    assert_eq!(
        g_cell.visibility,
        Visibility::Private,
        "priv param g : Length = 5mm where active (per-decl guard form) must compile to \
         Visibility::Private, got {:?}",
        g_cell.visibility
    );
}

// ── Part D coda: pins the enforcement-seam boundary from the scope note
// above with real (empirically verified) diagnostics, not just prose.
// EXTERNAL access on both member kinds is enforced (task #5171): `h.g`
// and `h.secret.main` each emit exactly one E_PRIV_MEMBER_ACCESS (see the
// `_emits_error` tests below), while their default-visible siblings still
// fail via their pre-existing, unrelated diagnostics — StructureMemberNotFound
// for guarded members, "member access not yet supported" for port members —
// unchanged. FUNCTION-BODY access is now enforced too: the skeleton template
// carries the port/guarded members, so both `m.secret.main` and `m.g` emit
// exactly one E_PRIV_MEMBER_ACCESS from a function body as well (see the
// `..._priv_gated` tests further below).

/// Shared `PortHost`+`Parent` fixture for the external port-member
/// enforcement tests below (reviewer_comprehensive suggestion #2): the priv
/// (`h.secret.main`) and pub (`h.plain.vis`) scenarios previously repeated
/// this source verbatim except for the trailing access expression, which
/// made it easy for the two to drift out of lockstep. Parameterizing on
/// `access_expr` keeps them structurally identical everywhere else.
fn port_host_external_access_source(access_expr: &str) -> String {
    format!(
        r#"
trait Iface {{}}

structure def PortHost {{
    port secret : Iface {{
        priv param main : Length = 5mm
    }}
    port plain : Iface {{
        param vis : Length = 5mm
    }}
}}

structure def Parent {{
    sub h = PortHost()
    let touch = {access_expr}
}}
"#
    )
}

/// External dot-access on a `priv param` nested inside a port now emits
/// E_PRIV_MEMBER_ACCESS (task #5171): a dedicated AST-pattern branch in
/// `compile_member_access` detects the `<sub>.<port>.<member>` shape before
/// `<sub>.<port>` is compiled down to a non-`StructureRef` receiver, and
/// poisons the access when the port's composite member cell
/// (`"<port>.<member>"`) is `Visibility::Private`.
#[test]
fn external_priv_port_member_access_emits_error() {
    let module = compile_source(&port_host_external_access_source("h.secret.main"));

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "external access to `h.secret.main` (priv port-member) must emit exactly one \
         E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
    assert!(
        priv_errs[0].message.contains("main"),
        "diagnostic should name the offending member: {}",
        priv_errs[0].message
    );
}

/// External dot-access on a default-visible port member sibling still
/// fails — but via the pre-existing generic "member access not yet
/// supported" diagnostic, not E_PRIV_MEMBER_ACCESS (pub port-member
/// resolution remains a separate, pre-existing gap that this task does not
/// close: the new AST-pattern branch only detects and poisons the priv
/// case).
#[test]
fn external_pub_port_member_access_not_yet_supported_unchanged() {
    let module = compile_source(&port_host_external_access_source("h.plain.vis"));

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `h.plain.vis` (default-visible port member) must NOT \
         emit E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let not_yet_supported = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("member access not yet supported"))
        .count();
    assert!(
        not_yet_supported >= 1,
        "external access to `h.plain.vis` must still fail with at least one generic \
         'member access not yet supported' diagnostic (pub port-member resolution is \
         unchanged by task #5171); all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Fail-closed regression guard (reviewer_comprehensive suggestion #1): the
/// new external port-member branch (expr.rs:~4007) explicitly excludes
/// collection subs via `!scope.collection_sub_names.contains(sub_name)`, so
/// an indexed access like `bolts[0].secret.main` on a `List<Bolt>` collection
/// sub structurally never reaches `port_member_is_priv` — the branch only
/// matches a bare `Ident` inner receiver, and `bolts[0]` is an `IndexAccess`,
/// so it falls through to the regular indexed-collection-member-access path
/// instead. That path resolves member names via `sub_member_types` (built
/// from `value_cells` only — ports are never included), so `secret` is
/// "unknown" and the access is poisoned there, before `.main` is ever
/// reached. This is a pre-existing, documented gap that #5171 does not close
/// (extending collection-sub port-member resolution is out of scope), but it
/// is NOT a silent leak: the access still fails via a real diagnostic. This
/// test pins both halves as a trip-wire — if a future change to the
/// collection-index member path ever made `bolts[0].secret.main` resolve
/// with zero diagnostics, this would catch the priv leak — mirroring the
/// existing `external_pub_port_member_access_not_yet_supported_unchanged` pin.
#[test]
fn external_priv_port_member_access_via_collection_index_not_yet_gated() {
    let module = compile_source(
        r#"
trait Iface {}

structure def Bolt {
    port secret : Iface {
        priv param main : Length = 5mm
    }
}

structure def Parent {
    sub bolts : List<Bolt>
    constraint bolts.count == 1
    let touch = bolts[0].secret.main
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `bolts[0].secret.main` (priv port-member on a collection sub) \
         must NOT emit E_PRIV_MEMBER_ACCESS via this path — collection subs are explicitly \
         excluded from the new port-member branch, a pre-existing gap #5171 does not close; \
         all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    // Assert the *specific* fail-closed diagnostic (not merely "some diagnostic"): a bare
    // `!is_empty()` check would stay green even if an unrelated diagnostic elsewhere in the
    // fixture masked a real priv leak on this path. This diagnostic has no DiagnosticCode
    // (emitted via a bare `Diagnostic::error(...)` at expr.rs's indexed-collection-member
    // branch, no `.with_code(...)`), so — mirroring the message-text count used by the
    // sibling `external_pub_port_member_access_not_yet_supported_unchanged` above — pin the
    // exact text instead: `bolts[0].secret` resolves through `sub_member_types` (built from
    // value_cells only; ports are absent), so `secret` is reported unknown before `.main` is
    // ever reached.
    let unknown_member_on_collection_sub = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown member 'secret' on collection sub 'bolts'"))
        .count();
    assert!(
        unknown_member_on_collection_sub >= 1,
        "external access to `bolts[0].secret.main` must still fail via the specific \
         \"unknown member 'secret' on collection sub 'bolts'\" diagnostic (fails closed), not \
         resolve silently with zero diagnostics or an unrelated one — a future change that \
         made this resolve cleanly would silently leak a priv port member; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A default (non-`pub`) `let` nested inside a `port { }` block also
/// compiles to `Visibility::Private` (entity.rs port-member `Let` arm), but
/// `let`s are never externally addressable by name, so `port_member_is_priv`
/// must NOT gate them — its `kind == ValueCellKind::Param` guard is what
/// excludes them. Regression-guards that load-bearing guard
/// (reviewer_comprehensive suggestion #1): if a future edit dropped the
/// `kind == Param` check, this would start failing.
#[test]
fn external_priv_let_in_port_not_gated() {
    let module = compile_source(
        r#"
trait Iface {}

structure def PortLetHost {
    port secret : Iface {
        let internal : Length = 5mm
    }
}

structure def Parent {
    sub h = PortLetHost()
    let touch = h.secret.internal
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `h.secret.internal` (default, non-pub `let` inside a port) must \
         NOT emit E_PRIV_MEMBER_ACCESS — `let`s default to Visibility::Private but are never \
         externally addressable by name, so `port_member_is_priv`'s `kind == Param` guard must \
         exclude them; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Internal access to a structure's own `priv` port member — the bare
/// `<port>.<member>` path (task #3978 δ, expr.rs:~3912), NOT
/// `self.<port>.<member>` — stays ungated: priv hides members from OUTSIDE
/// the defining scope only. The new external-access AST-pattern branch
/// (task #5171) requires a non-`self` sub receiver bound in
/// `sub_component_types`, so it structurally cannot fire on this
/// single-level shape; this test pins that with a real diagnostic count.
/// Shared `PortHostSelf` fixture for the internal port-member access tests
/// below (reviewer_comprehensive suggestion #2): the bare-`port.member` and
/// explicit-`self.port.member` scenarios previously repeated this source
/// verbatim except for the access expression itself. Parameterizing on
/// `access_expr` keeps them structurally identical everywhere else.
fn port_host_self_source(access_expr: &str) -> String {
    format!(
        r#"
trait Iface {{}}

structure def PortHostSelf {{
    port secret : Iface {{
        priv param main : Length = 5mm
    }}
    let echo = {access_expr}
}}
"#
    )
}

#[test]
fn internal_priv_port_member_access_ok() {
    let module = compile_source(&port_host_self_source("secret.main"));

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "internal access to `secret.main` (own priv port member, bare port.member path) \
         must NOT emit E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Internal access to a structure's own `priv` port member via the explicit
/// two-level `self.<port>.<member>` shape — as opposed to the bare
/// `<port>.<member>` path pinned above — also stays ungated. The new
/// external-access AST-pattern branch (task #5171) only fires when the
/// receiver's inner identifier is a *non-`self`* name bound in
/// `sub_component_types`, so it structurally cannot fire here; this test
/// pins that specifically for the explicit-`self` shape, so a future change
/// to how `self.<port>` resolves cannot start silently leaking
/// E_PRIV_MEMBER_ACCESS gating (or a priv bypass) on this path without a
/// test noticing.
#[test]
fn internal_priv_port_member_access_via_self_ok() {
    let module = compile_source(&port_host_self_source("self.secret.main"));

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "internal access to `self.secret.main` (own priv port member, explicit \
         self.<port>.<member> path) must NOT emit E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// External dot-access on a `priv param` nested inside a block-form guarded
/// group now emits E_PRIV_MEMBER_ACCESS (task #5171): `template_member_is_priv`
/// scans `guarded_groups[].members`, and the E_PRIV_MEMBER_ACCESS check is
/// reordered to run before the StructureMemberNotFound early-return (a priv
/// guarded-block member is never `member_known`, since `template_has_member`
/// doesn't scan `guarded_groups`).
/// Shared `GuardHost`+`Parent` fixture for the external guarded-block-member
/// enforcement tests below (reviewer_comprehensive suggestion #2): the priv
/// (`h.g`) and pub (`h.vis`) scenarios previously repeated this source
/// verbatim except for the trailing access expression. Parameterizing on
/// `access_expr` keeps them structurally identical everywhere else.
fn guard_host_external_access_source(access_expr: &str) -> String {
    format!(
        r#"
structure def GuardHost {{
    param active : Bool = true
    where active {{
        priv param g : Length = 5mm
        param vis : Length = 5mm
    }}
}}

structure def Parent {{
    sub h = GuardHost()
    let touch = {access_expr}
}}
"#
    )
}

#[test]
fn external_priv_guarded_member_access_emits_error() {
    let module = compile_source(&guard_host_external_access_source("h.g"));

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "external access to `h.g` (priv guarded-block member) must emit exactly one \
         E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
    assert!(
        // Quoted full-token form (not a bare `contains('g')` substring check):
        // a single unquoted character could incidentally match unrelated text
        // in a reworded diagnostic (e.g. the 'g' in "GuardHost") and would not
        // meaningfully pin the offending member's identity.
        priv_errs[0].message.contains("'g'"),
        "diagnostic should name the offending member: {}",
        priv_errs[0].message
    );
}

/// External dot-access on a default-visible guarded-block member sibling
/// still fails — but via E_STRUCTURE_MEMBER_NOT_FOUND, not
/// E_PRIV_MEMBER_ACCESS (pub guarded-member resolution remains a separate,
/// pre-existing gap that this task does not close: `template_has_member`
/// still doesn't scan `guarded_groups`).
#[test]
fn external_pub_guarded_member_access_not_found_unchanged() {
    let module = compile_source(&guard_host_external_access_source("h.vis"));

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `h.vis` (default-visible guarded-block member) must NOT \
         emit E_PRIV_MEMBER_ACCESS; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let not_found = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StructureMemberNotFound))
        .count();
    assert!(
        not_found >= 1,
        "external access to `h.vis` must still fail via E_STRUCTURE_MEMBER_NOT_FOUND \
         (pub guarded-member resolution is unchanged by task #5171); all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A default (non-`pub`) `let` nested inside a block-form `where cond { }`
/// guarded group also compiles to `Visibility::Private`
/// (guards.rs `compile_guarded_members`'s `Let` arm), but `let`s are never
/// externally addressable by name, so `template_member_is_priv`'s
/// `guarded_groups[].members` scan must NOT gate them — its
/// `kind == ValueCellKind::Param` guard is what excludes them.
/// Regression-guards that load-bearing guard (reviewer_comprehensive
/// suggestion #1): if a future edit dropped the `kind == Param` check, this
/// would start failing.
#[test]
fn external_priv_let_in_guarded_block_not_gated() {
    let module = compile_source(
        r#"
structure def GuardLetHost {
    param active : Bool = true
    where active {
        let internal : Length = 5mm
    }
}

structure def Parent {
    sub h = GuardLetHost()
    let touch = h.internal
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "external access to `h.internal` (default, non-pub `let` inside a guarded block) must \
         NOT emit E_PRIV_MEMBER_ACCESS — `let`s default to Visibility::Private but are never \
         externally addressable by name, so `template_member_is_priv`'s guarded_groups scan \
         must exclude them via the `kind == Param` guard; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Part D coda, function-body variant — skeleton priv gating ────────────────
//
// `build_structure_def_skeleton` (entity.rs) — the template consulted while
// type-checking function bodies, before per-structure templates exist — now
// populates `ports` and `guarded_groups` with the nested `priv`-aware members
// (task #5161's lowering shape, kept lightweight). So a function body reading a
// `priv` port member (`m.secret.main`) or a `priv` guarded-block member (`m.g`)
// is priv-gated exactly like external access: each emits one
// E_PRIV_MEMBER_ACCESS. The guarded case is gated by the skeleton population
// alone (the SIR-α `StructureRef` E_PRIV gate scans `guarded_groups`); the port
// case additionally needs the `expr.rs` receiver-resolution branch that resolves
// a fn-body `StructureRef` receiver (ports are absent from `value_cells`, so
// `m.secret` never types as a `StructureRef`). The tests below pin both.

/// A function body accessing a `priv` param nested inside a `port { }` block is
/// now priv-gated: `build_structure_def_skeleton` populates the skeleton's
/// `ports` (composite `"<port>.<member>"` cells) and the `expr.rs`
/// `<sub>.<port>.<member>` branch resolves the fn-body `StructureRef` receiver,
/// so `port_member_is_priv` fires exactly one E_PRIV_MEMBER_ACCESS on
/// `m.secret.main`.
#[test]
fn function_body_priv_port_member_access_priv_gated() {
    let module = compile_source(
        r#"
trait Iface {}

structure def PortHost {
    port secret : Iface {
        priv param main : Length = 5mm
    }
}

fn leak(m : PortHost) -> Length { m.secret.main }
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "function-body access to a priv port-member (`m.secret.main`) must emit \
         exactly one E_PRIV_MEMBER_ACCESS — the skeleton now carries the port's \
         members and the expr.rs branch resolves the fn-body receiver; all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
}

/// A function body reading a *bare, non-priv* port (`m.secret`, where `secret`
/// is a default-visible `port { }`) is NOT priv-gated, and — now that the
/// skeleton populates `ports` — is treated as a KNOWN member: it resolves to the
/// permissive dimensionless-scalar fallback rather than failing with
/// E_STRUCTURE_MEMBER_NOT_FOUND. This pins the (intended) blast radius of the
/// skeleton `ports` population: `template_has_member` now sees the port name, so
/// a bare non-priv port access from a fn body aligns with the authoritative
/// `StructureRef` path (which already treats port names as known members)
/// instead of the pre-change member-not-found behaviour. It also guards the
/// symmetric non-priv counterpart of the priv-port gate above: populating `ports`
/// must not over-fire E_PRIV_MEMBER_ACCESS on a default-visible port.
#[test]
fn function_body_pub_port_access_resolves_not_priv_gated() {
    let module = compile_source(
        r#"
trait Iface {}

structure def PortHost {
    port secret : Iface {
        param main : Length = 5mm
    }
}

fn ok(m : PortHost) -> Real { m.secret }
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "function-body access to a bare non-priv port (`m.secret`) must NOT emit \
         E_PRIV_MEMBER_ACCESS — the port is default-visible; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let not_found = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StructureMemberNotFound))
        .count();
    assert_eq!(
        not_found, 0,
        "function-body access to a bare non-priv port (`m.secret`) must NOT emit \
         E_STRUCTURE_MEMBER_NOT_FOUND — populating the skeleton's `ports` makes the \
         port name a KNOWN member (template_has_member scans ports), so the access \
         resolves to the dimensionless fallback instead of failing not-found; all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A function body accessing a `priv` param nested inside a block-form
/// `where cond { }` guarded group is now priv-gated:
/// `build_structure_def_skeleton` populates the skeleton template's
/// `guarded_groups`, so the SIR-α `StructureRef` member-access block's
/// pre-ordered E_PRIV gate (`template_member_is_priv`'s `guarded_groups` scan)
/// fires exactly one E_PRIV_MEMBER_ACCESS on `m.g`. No expr.rs change is
/// needed for the guarded-member case — populating the skeleton is sufficient.
#[test]
fn function_body_priv_guarded_member_access_priv_gated() {
    let module = compile_source(
        r#"
structure def GuardHost {
    param active : Bool = true
    where active {
        priv param g : Length = 5mm
    }
}

fn leak(m : GuardHost) -> Length { m.g }
"#,
    );

    let priv_errs = priv_access_errors(&module);
    assert_eq!(
        priv_errs.len(),
        1,
        "function-body access to a priv guarded-block member (`m.g`) must emit \
         exactly one E_PRIV_MEMBER_ACCESS — the skeleton template now carries the \
         guarded group's members so the priv gate fires; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
}

// ── Part C: deep-chain enforcement + the `self.<sub>.<member>` bypass ─────────
//
// Added by task #5424 (α of PRD docs/prds/v0_6/uniform-member-access.md), which
// introduced `member_path` as the single member-shape authority and routed both
// external-access priv verdicts through it. These pin the FLOOR that α's
// successors (#5425 β, #5430 η) must not regress: `priv` is enforced at EVERY
// hop of a dotted chain and attributed to the concrete structure at that hop —
// plus, deliberately, the one shape where it still is not.

/// Boundary row 7 — a `priv param` at the END of a two-hop chain is denied, and
/// the diagnostic names `Leaf` (the structure the hop was taken FROM), not the
/// chain root `Mid`.
#[test]
fn deep_chain_priv_param_is_denied_and_attributed_to_the_concrete_structure() {
    let module = compile_source(
        r#"
structure def Leaf {
    priv param secret : Length = 5mm
    param plain : Length = 6mm
}

structure def Mid {
    let leaf = Leaf()
}

structure def Test {
    let m = Mid()
    let deep = m.leaf.secret
}
"#,
    );

    let messages: Vec<&str> = priv_access_errors(&module)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec!["E_PRIV_MEMBER_ACCESS: member 'secret' of structure 'Leaf' is private"],
        "exactly one E_PRIV_MEMBER_ACCESS, attributed to Leaf (the concrete \
         structure at that hop) rather than to the chain root Mid; all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Boundary row 7 — enforcement at a NON-terminal hop: a `priv sub` in the
/// middle of the chain is denied there, so the walk never reaches the (public)
/// member behind it.
#[test]
fn deep_chain_priv_sub_is_denied_at_the_intermediate_hop() {
    let module = compile_source(
        r#"
structure def Leaf {
    param open : Length = 6mm
}

structure def Mid {
    priv sub leaf = Leaf()
}

structure def Test {
    let m = Mid()
    let deep = m.leaf.open
}
"#,
    );

    let messages: Vec<&str> = priv_access_errors(&module)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec!["E_PRIV_MEMBER_ACCESS: member 'leaf' of structure 'Mid' is private"],
        "the priv verdict must fire at the INTERMEDIATE hop — `open` is public, \
         so a resolver that only checked the terminal would let this through; \
         all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// The controls for the two pins above: with everything public, the same chain
/// shapes emit ZERO priv diagnostics. Without these, both pins could pass
/// vacuously off an unrelated blanket denial.
#[test]
fn deep_chain_public_members_emit_no_priv_diagnostic() {
    let public_sub = compile_source(
        r#"
structure def Leaf {
    param open : Length = 6mm
}

structure def Mid {
    sub leaf = Leaf()
}

structure def Test {
    let m = Mid()
    let deep = m.leaf.open
}
"#,
    );
    assert_eq!(
        priv_access_errors(&public_sub).len(),
        0,
        "a public sub at the intermediate hop must not be denied; all \
         diagnostics: {:?}",
        public_sub.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let public_terminal = compile_source(
        r#"
structure def Leaf {
    priv param secret : Length = 5mm
    param plain : Length = 6mm
}

structure def Mid {
    let leaf = Leaf()
}

structure def Test {
    let m = Mid()
    let deep = m.leaf.plain
}
"#,
    );
    assert_eq!(
        priv_access_errors(&public_terminal).len(),
        0,
        "a public terminal member alongside a priv sibling must not be denied; \
         all diagnostics: {:?}",
        public_terminal.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Follow-up task that closes the `self.<sub>.<member>` priv bypass pinned
/// below. When #5430 (η) routes the two-level `self.<sub>.<member>` matchers
/// through `member_path`, grep this file for `SELF_SUB_BYPASS_FOLLOWUP` and flip
/// the assertion from "expect 0 E_PRIV_MEMBER_ACCESS" to "expect exactly 1".
const SELF_SUB_BYPASS_FOLLOWUP: &str = "#5430";

/// PINNED BYPASS (esc-5424-3) — `self.<sub>.<priv member>` reads the value with
/// ZERO diagnostics.
///
/// # Why this pin is stronger than its two siblings
///
/// `external_priv_port_member_access_via_collection_index_not_yet_gated` and
/// `external_priv_let_in_port_not_gated` are tolerable gaps because the access
/// still FAILS CLOSED through an unrelated diagnostic. This one does not: the
/// shape compiles clean and `reify eval` prints `Test.leak = 0.005 m`. So the
/// assertion has to name the leak explicitly — zero `PrivMemberAccess` AND zero
/// error-severity diagnostics at all — rather than "some diagnostic fired".
///
/// # Why it is a real bug, not a design choice
///
/// It is shape-specific and violates the `self.X` ≡ `X` equivalence (spec §8.6,
/// task 4615): the identical read without the `self.` prefix (`i.secret`) and
/// the `let`-instance form (`m.secret`) BOTH correctly emit
/// `E_PRIV_MEMBER_ACCESS`. Only the two-level `self.<sub>.<member>` matcher
/// misses it, because that path never produces a `Type::StructureRef` receiver
/// for the outer `.secret`, so the external-access priv gate is never entered.
///
/// # Why task #5424 (α) does not fix it
///
/// `ExprScope` carries only flattened `name → Type` maps with no visibility
/// axis, so a point fix needs a parallel `sub_member_visibility` map — exactly
/// the lockstep duplication contract C1-iv / INV-5 forbids, and exactly what
/// #5430 (η) exists to delete by routing this site through
/// `member_path::resolve_member_path`.
///
/// The pin exists to turn an invisible landmine into a loud mechanical signal:
/// η's implementer will see this test fail and read the new diagnostic as the
/// intended fix, not as a regression they introduced.
#[test]
fn self_sub_priv_member_access_is_not_gated_pinned_bypass() {
    let module = compile_source(
        r#"
structure def Inner {
    priv param secret : Length = 5mm
}

structure def Test {
    sub i = Inner()
    let leak = self.i.secret
}
"#,
    );

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "`self.i.secret` does not reach the external-access priv gate today \
         (tracked by {SELF_SUB_BYPASS_FOLLOWUP}); when that lands, FLIP this to \
         expect exactly one E_PRIV_MEMBER_ACCESS rather than weakening it; all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    // The load-bearing half: unlike the two sibling pins above, this access does
    // NOT fail closed. It resolves cleanly and hands back the priv value, so the
    // pin must assert the leak itself.
    let errors: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        errors.is_empty(),
        "this is a SILENT leak, not a fail-closed gap: the fixture compiles with \
         zero errors and evaluates `Test.leak` to the priv value. If errors have \
         appeared here, the bypass has been closed — flip the assertion above \
         instead of relaxing this one; errors: {errors:?}"
    );
}
