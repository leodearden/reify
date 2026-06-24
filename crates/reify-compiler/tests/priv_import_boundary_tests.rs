//! Cross-module priv-import boundary test (task #3978 δ — step 7/8, PRD §6).
//!
//! Proves the §6 two-way visibility signal: a `priv` member of an imported
//! `pub` def stays hidden across the module boundary. A consumer that imports a
//! `pub structure def Motor { priv param p; param q }` can read the
//! default-visible `m.q` but a dot-access to the `priv` member `m.p` emits
//! `E_PRIV_MEMBER_ACCESS`.
//!
//! This falls out of the same `expr.rs` enforcement used in-module (step-6):
//! `merge_imported_pub_templates` clones whole templates (members + per-member
//! visibility) into the consumer's resolution registry, so the priv marker
//! survives the import and is gated at the shared member-access check.

use std::fs;

use reify_compiler::module_dag::{compile_project, ModuleResolver};
use reify_compiler::CompiledModule;
use reify_core::{Diagnostic, DiagnosticCode};

/// Flatten every diagnostic from a `compile_project` result — the priv-access
/// diagnostic may surface either on a returned module (`Ok`) or in the
/// hard-error vec (`Err`).
fn all_diagnostics(result: Result<Vec<CompiledModule>, Vec<Diagnostic>>) -> Vec<Diagnostic> {
    match result {
        Ok(mods) => mods.into_iter().flat_map(|m| m.diagnostics).collect(),
        Err(diags) => diags,
    }
}

fn priv_access_errors(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::PrivMemberAccess))
        .collect()
}

/// A `priv` member of an imported `pub` def stays hidden across the module
/// boundary; the default-visible member resolves with no error.
#[test]
fn priv_member_of_imported_pub_def_stays_hidden() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    fs::write(
        dir.join("producer.ri"),
        "pub structure def Motor {\n\
         \x20   priv param p : Real = 0\n\
         \x20   param q : Real = 0\n\
         }\n",
    )
    .unwrap();

    fs::write(
        dir.join("consumer.ri"),
        "import producer\n\
         \n\
         structure def App {\n\
         \x20   sub m = Motor()\n\
         \x20   let visible = m.q\n\
         \x20   let hidden = m.p\n\
         }\n",
    )
    .unwrap();

    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    let diags = all_diagnostics(compile_project(&dir.join("consumer.ri"), &resolver));

    let priv_errs = priv_access_errors(&diags);
    assert_eq!(
        priv_errs.len(),
        1,
        "exactly one E_PRIV_MEMBER_ACCESS expected for the cross-module `m.p` access; \
         all diagnostics: {:?}",
        diags
            .iter()
            .map(|d| format!("{}: {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
    assert!(priv_errs[0].message.contains("E_PRIV_MEMBER_ACCESS"));
    assert!(
        priv_errs[0].message.contains('p'),
        "the diagnostic should name the priv member `p`, not the visible `q`: {}",
        priv_errs[0].message
    );
}
