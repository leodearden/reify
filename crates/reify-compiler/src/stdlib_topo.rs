//! Topological sort for the embedded stdlib prelude.
//!
//! Provides [`compile_modules_topo`] — an in-memory, stable topological sort
//! over a set of `(module_name, source)` pairs, using `Declaration::Import`
//! edges for ordering and a gray/black DFS for cycle detection.

#[cfg(test)]
mod tests {
    use reify_core::{ModulePath, Severity};
    use crate::CompiledModule;
    use super::compile_modules_topo;

    /// SIGNAL #1: a shared surface type declared AFTER its consumers in input order
    /// is reachable from both early and late consumers when compiled via
    /// `compile_modules_topo`.
    ///
    /// Companion assertion: naive compilation in input order (early before shared)
    /// produces an Error on `early` — proving the topo mechanism is load-bearing.
    #[test]
    fn signal_1_shared_surface_type_reachable_from_early_and_late() {
        let early_src = "import shared\n\nstructure def E {\n    param v : Vec3\n}";
        let shared_src = "pub type Vec3 = Real";
        let late_src = "import shared\n\nstructure def L {\n    param w : Vec3\n}";

        // Input order: early BEFORE shared (naive compilation fails for early)
        let set: &[(&str, &str)] = &[
            ("early", early_src),
            ("shared", shared_src),
            ("late", late_src),
        ];

        // Topo path must succeed
        let modules = compile_modules_topo(set)
            .expect("compile_modules_topo must return Ok for a DAG with no cycle");

        // No Error-severity diagnostics across any returned module
        for module in &modules {
            let errors: Vec<_> = module
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "module '{}' has Error-severity diagnostics via topo path: {:?}",
                module.path,
                errors
            );
        }

        // `early` specifically resolves Vec3 (no Error mentioning Vec3)
        let early_mod = modules
            .iter()
            .find(|m| format!("{}", m.path) == "early")
            .expect("expected 'early' in topo result");
        let vec3_errors: Vec<_> = early_mod
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("Vec3"))
            .collect();
        assert!(
            vec3_errors.is_empty(),
            "'early' has Vec3-related Error diagnostics via topo path: {:?}",
            vec3_errors
        );

        // COMPANION: naive input-order compilation makes `early` fail Vec3 resolution
        let mut naive_modules: Vec<CompiledModule> = Vec::new();
        for (module_name, source) in set {
            let prelude_enum_names: Vec<&str> = naive_modules
                .iter()
                .flat_map(|m: &CompiledModule| m.enum_defs.iter().map(|e| e.name.as_str()))
                .collect();
            let parsed = reify_syntax::parse_with_prelude_enums(
                source,
                ModulePath::from_dotted(module_name).expect("valid dotted path"),
                &prelude_enum_names,
            );
            let compiled = crate::compile_with_prelude(&parsed, &naive_modules);
            naive_modules.push(compiled);
        }
        let naive_early = naive_modules
            .iter()
            .find(|m| format!("{}", m.path) == "early")
            .expect("expected 'early' in naive result");
        let naive_errors: Vec<_> = naive_early
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !naive_errors.is_empty(),
            "naive compilation: expected 'early' to have Error-severity diagnostics \
             (Vec3 unresolved before shared), but got none — \
             the topo mechanism would then be non-load-bearing"
        );
    }
}
