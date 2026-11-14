//! The function table a scope resolves calls against, together with which of
//! its same-module signature stubs cannot pad an omitted default yet (task
//! 5494, PRD `stdlib-namespace.md` D-8).
//!
//! A leaf: it names no other module of this crate, so `scope`, `functions` and
//! the compile phases all import it downward.

use std::collections::{HashMap, HashSet};

use reify_core::Type;
use reify_ir::CompiledFunction;

/// The same-module signature stubs whose param defaults wait for a same-module
/// structure template (`functions::FnParamDefaults::NeedLocalTemplates`).
///
/// Keyed by OVERLOAD identity, name plus params, so a same-name overload with
/// compiled defaults, or with none declared, is never taken for a pending one.
/// Each entry holds the params that DECLARE a default: exactly the slots its
/// stub leaves `None` that the real fn will fill.
///
/// Describes the stubs of the table it travels with ([`CallTable`]): once an
/// overload's real fn replaces its stub, [`Self::resolve`] takes it out.
#[derive(Debug, Default)]
pub(crate) struct PendingDefaults {
    by_name: HashMap<String, Vec<PendingOverload>>,
}

#[derive(Debug)]
struct PendingOverload {
    params: Vec<(String, Type)>,
    defaulted: HashSet<String>,
}

impl PendingDefaults {
    /// Record the overload `name(params)` as a stub whose `defaulted` params
    /// cannot pad yet.
    pub(crate) fn insert(
        &mut self,
        name: &str,
        params: &[(String, Type)],
        defaulted: HashSet<String>,
    ) {
        self.by_name
            .entry(name.to_string())
            .or_default()
            .push(PendingOverload {
                params: params.to_vec(),
                defaulted,
            });
    }

    /// `real` has replaced its stub in the table: it pads like any other fn.
    pub(crate) fn resolve(&mut self, real: &CompiledFunction) {
        if let Some(overloads) = self.by_name.get_mut(&real.name) {
            overloads.retain(|o| o.params != real.params);
            if overloads.is_empty() {
                self.by_name.remove(&real.name);
            }
        }
    }

    /// The defaulted params `cand` cannot pad yet, when `cand` is a pending
    /// overload's stub; `None` for every other fn.
    pub(crate) fn defaulted_params_of(&self, cand: &CompiledFunction) -> Option<&HashSet<String>> {
        self.by_name
            .get(&cand.name)?
            .iter()
            .find(|o| o.params == cand.params)
            .map(|o| &o.defaulted)
    }
}

/// A call-resolution table plus which of its same-module stubs cannot pad yet.
/// The two travel together, so every scope that resolves calls against a table
/// holding such a stub is told which ones.
#[derive(Clone, Copy)]
pub(crate) struct CallTable<'a> {
    pub(crate) functions: &'a [CompiledFunction],
    pub(crate) fns_with_pending_defaults: Option<&'a PendingDefaults>,
}

impl<'a> CallTable<'a> {
    /// A table whose stubs, if any, carry every default they will ever have.
    pub(crate) fn complete(functions: &'a [CompiledFunction]) -> Self {
        CallTable {
            functions,
            fns_with_pending_defaults: None,
        }
    }

    /// A table that may hold the stubs of `pending` fns.
    pub(crate) fn with_pending(
        functions: &'a [CompiledFunction],
        pending: &'a PendingDefaults,
    ) -> Self {
        CallTable {
            functions,
            fns_with_pending_defaults: Some(pending),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub(name: &str, params: &[(&str, Type)]) -> CompiledFunction {
        let params: Vec<(String, Type)> = params
            .iter()
            .map(|(n, t)| (n.to_string(), t.clone()))
            .collect();
        CompiledFunction {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::dimensionless_scalar(),
            body: reify_ir::CompiledFnBody {
                let_bindings: Vec::new(),
                result_expr: reify_ir::CompiledExpr::literal(
                    reify_ir::Value::Real(0.0),
                    Type::dimensionless_scalar(),
                ),
            },
            content_hash: reify_core::ContentHash::of_str(name),
            annotations: Vec::new(),
            optimized_target: None,
            type_params: Vec::new(),
        }
    }

    fn real_opts() -> Vec<(&'static str, Type)> {
        vec![
            ("x", Type::dimensionless_scalar()),
            ("o", Type::StructureRef("Opts".to_string())),
        ]
    }

    fn pending_solve() -> PendingDefaults {
        let pending_fn = stub("solve", &real_opts());
        let mut pending = PendingDefaults::default();
        pending.insert(
            "solve",
            &pending_fn.params,
            HashSet::from(["o".to_string()]),
        );
        pending
    }

    #[test]
    fn the_pending_overload_reports_its_defaulted_params() {
        let pending = pending_solve();
        let defaulted = pending.defaulted_params_of(&stub("solve", &real_opts()));
        assert_eq!(defaulted, Some(&HashSet::from(["o".to_string()])));
    }

    #[test]
    fn a_same_name_overload_with_other_params_is_not_pending() {
        let pending = pending_solve();
        let string_overload = stub(
            "solve",
            &[("x", Type::String), ("o", Type::dimensionless_scalar())],
        );
        assert_eq!(pending.defaulted_params_of(&string_overload), None);
    }

    #[test]
    fn an_overload_is_no_longer_pending_once_its_real_fn_resolves_it() {
        let mut pending = pending_solve();
        let real = stub("solve", &real_opts());
        pending.resolve(&real);
        assert_eq!(pending.defaulted_params_of(&real), None);
    }

    #[test]
    fn resolving_one_overload_keeps_its_pending_sibling() {
        let mut pending = pending_solve();
        let sibling = stub("solve", &[("x", Type::String), ("o", Type::String)]);
        pending.insert("solve", &sibling.params, HashSet::from(["o".to_string()]));
        pending.resolve(&stub("solve", &real_opts()));
        assert!(pending.defaulted_params_of(&sibling).is_some());
        assert_eq!(
            pending.defaulted_params_of(&stub("solve", &real_opts())),
            None
        );
    }
}
