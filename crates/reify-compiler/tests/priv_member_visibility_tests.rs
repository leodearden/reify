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
// access still can't reach either member kind at all — the composite
// port/guarded member paths aren't wired into `build_structure_def_skeleton`
// — so that slice of the gap remains open, now tracked by follow-up #5222
// (see the `..._not_yet_priv_gated` tests further below). Part D itself
// asserts only the lowered `visibility` field, not dot-access enforcement,
// so it stays green independent of either follow-up's status.

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
// EXTERNAL access on both member kinds is now enforced (task #5171): `h.g`
// and `h.secret.main` each emit exactly one E_PRIV_MEMBER_ACCESS (see the
// `_emits_error` tests below), while their default-visible siblings still
// fail via their pre-existing, unrelated diagnostics — StructureMemberNotFound
// for guarded members, "member access not yet supported" for port members —
// unchanged. FUNCTION-BODY access is the remaining gap: neither member kind
// resolves at all there, so the priv gate is still never reached (see the
// `..._not_yet_priv_gated` tests further below, tracked by follow-up #5222).

/// Follow-up task tracking the remaining function-body/skeleton priv-gate
/// enforcement gap pinned by the `..._not_yet_priv_gated` tests below
/// (EXTERNAL-access enforcement landed via #5171). When #5222 lands
/// (populating `build_structure_def_skeleton`'s `ports`/`guarded_groups` so
/// function bodies can resolve these members), grep this file for
/// `NOT_YET_PRIV_GATED_FOLLOWUP` to find every assertion that must flip from
/// "expect 0 E_PRIV_MEMBER_ACCESS" to "expect exactly 1".
const NOT_YET_PRIV_GATED_FOLLOWUP: &str = "#5222";

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
        priv_errs[0].message.contains('g'),
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

// ── Part D coda, function-body variant — skeleton Public-by-omission ──────────
//
// `build_structure_def_skeleton` (entity.rs) — the template used while
// type-checking function bodies, before per-structure templates exist —
// unconditionally returns `ports: vec![]` and `guarded_groups: vec![]`, so
// task #5161's priv-aware lowering never runs for it. This is harmless
// today (verified below): an empty `ports`/`guarded_groups` vec means the
// port or guarded member isn't found on the skeleton, so function-body
// access fails at E_STRUCTURE_MEMBER_NOT_FOUND before any visibility check
// — same failure mode and root cause as the external-access coda above had,
// before #5171 landed EXTERNAL-access enforcement there. The function-body
// path is unaffected by that landing (the skeleton itself is untouched) and
// remains open, tracked by follow-up #5222. No silent success, no new leak.

/// A function body cannot reach a `priv` param nested inside a `port { }`
/// block via the skeleton registry — the skeleton's `ports` vec is always
/// empty, so the port itself is unresolved (E_STRUCTURE_MEMBER_NOT_FOUND),
/// not silently granted access.
#[test]
fn function_body_priv_port_member_access_not_yet_priv_gated() {
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

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "function-body access to a priv port-member must not emit \
         E_PRIV_MEMBER_ACCESS today — the skeleton template never carries port \
         members (tracked by {NOT_YET_PRIV_GATED_FOLLOWUP}); all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let not_found = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StructureMemberNotFound))
        .count();
    // `>= 1` rather than `== 1`: the load-bearing claim is that access fails
    // closed (via E_STRUCTURE_MEMBER_NOT_FOUND, not silently), not that the
    // compiler emits precisely one such diagnostic for this path. An
    // unrelated future change adding a second, legitimately different
    // member-resolution diagnostic on this fixture shouldn't fail this test.
    assert!(
        not_found >= 1,
        "function-body access to `m.secret` must fail with at least one \
         E_STRUCTURE_MEMBER_NOT_FOUND (the skeleton's empty `ports` vec means \
         the port itself is unresolved) — this pins that the access fails \
         closed, not open; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A function body cannot reach a `priv` param nested inside a block-form
/// `where cond { }` guarded group via the skeleton registry — the skeleton's
/// `guarded_groups` vec is always empty, so the member is unresolved
/// (E_STRUCTURE_MEMBER_NOT_FOUND), not silently granted access.
#[test]
fn function_body_priv_guarded_member_access_not_yet_priv_gated() {
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

    assert_eq!(
        priv_access_errors(&module).len(),
        0,
        "function-body access to a priv guarded-block member must not emit \
         E_PRIV_MEMBER_ACCESS today — the skeleton template never carries \
         guarded-group members (tracked by {NOT_YET_PRIV_GATED_FOLLOWUP}); all \
         diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let not_found = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StructureMemberNotFound))
        .count();
    // `>= 1` rather than `== 1`: the load-bearing claim is that access fails
    // closed (via E_STRUCTURE_MEMBER_NOT_FOUND, not silently), not that the
    // compiler emits precisely one such diagnostic for this path. An
    // unrelated future change adding a second, legitimately different
    // member-resolution diagnostic on this fixture shouldn't fail this test.
    assert!(
        not_found >= 1,
        "function-body access to `m.g` must fail with at least one \
         E_STRUCTURE_MEMBER_NOT_FOUND (the skeleton's empty `guarded_groups` \
         vec means the member is unresolved) — this pins that the access fails \
         closed, not open; all diagnostics: {:?}",
        module.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
