// Split from lib.rs (task 2032) — purposes methods.

use reify_types::{ConstraintNodeId, ContentHash, OptimizationObjective};
use crate::cache::NodeId;
use crate::deps::ReverseDependencyIndex;
use crate::Engine;

impl Engine {
    /// Activate a purpose by name against a target entity.
    ///
    /// Looks up the compiled purpose by `purpose_name`, then injects its
    /// constraints into the current evaluation graph. The injected constraint
    /// node IDs use a `"purpose:<purpose_name>@<entity_ref>"` entity prefix to
    /// avoid collisions with structure-level constraints.
    ///
    /// Requires a prior call to `eval()` so an evaluation state exists.
    /// If the purpose is already active, this is a no-op.
    pub fn activate_purpose(&mut self, purpose_name: &str, entity_ref: &str) {
        // No-op if already active
        if self.active_purposes.contains_key(purpose_name) {
            return;
        }

        // Look up the compiled purpose
        let purpose = match self
            .compiled_purposes
            .iter()
            .find(|p| p.name == purpose_name)
        {
            Some(p) => p.clone(),
            None => return, // Purpose not found — silently ignore
        };

        // Get mutable access to the evaluation state
        let state = match self.eval_state.as_mut() {
            Some(s) => s,
            None => return, // No eval state — silently ignore
        };

        // Build a unique entity prefix for the purpose-injected constraints
        let purpose_entity = format!("purpose:{}@{}", purpose_name, entity_ref);

        // Rewrite compiled expressions: substitute ValueCellId(purpose_name, param)
        // with ValueCellId(entity_ref, param) so references resolve to existing
        // value cells in the evaluation graph.
        let mut rewritten_constraints = purpose.constraints.clone();
        for constraint in &mut rewritten_constraints {
            constraint.expr.remap_entity(purpose_name, entity_ref);
        }

        let rewritten_objective = purpose.objective.clone().map(|mut obj| {
            match &mut obj {
                OptimizationObjective::Minimize(expr) | OptimizationObjective::Maximize(expr) => {
                    expr.remap_entity(purpose_name, entity_ref);
                }
            }
            obj
        });

        // Inject each of the purpose's compiled constraints into the evaluation graph
        let mut injected_ids = Vec::new();
        for (i, constraint) in rewritten_constraints.iter().enumerate() {
            let constraint_id = ConstraintNodeId::new(&purpose_entity, i as u32);
            let node = crate::graph::ConstraintNodeData {
                id: constraint_id.clone(),
                label: constraint.label.clone(),
                expr: constraint.expr.clone(),
                content_hash: ContentHash::of_str(&format!(
                    "purpose:{}:constraint:{}",
                    purpose_name, i
                )),
                optimized_target: constraint.optimized_target.clone(),
            };
            state
                .snapshot
                .graph
                .constraints
                .insert(constraint_id.clone(), node);
            injected_ids.push(constraint_id);
        }

        // Update demand registry: demand each newly injected constraint node.
        for id in &injected_ids {
            self.demand.add_demand(NodeId::Constraint(id.clone()));
        }

        self.active_purposes
            .insert(purpose_name.to_string(), injected_ids);

        // Rebuild infrastructure so incremental eval (edit_param) propagates
        // through purpose constraint dependencies correctly.
        //
        // We reborrow eval_state mutably here — the immutable borrow (`state`)
        // created earlier was already released after inserting into the graph.
        if let Some(state) = self.eval_state.as_mut() {
            state.reverse_index = ReverseDependencyIndex::build_from_graph(&state.snapshot.graph);
            state.trace_map = crate::deps::build_trace_map(&state.snapshot.graph);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Inject the optimization objective if the purpose has one
        if let Some(ref objective) = rewritten_objective {
            self.active_objective_map
                .insert(purpose_name.to_string(), objective.clone());
        }
    }

    /// Deactivate a purpose by name.
    ///
    /// Removes the constraints and objectives that were injected by `activate_purpose`.
    /// If the purpose is not active, this is a no-op.
    pub fn deactivate_purpose(&mut self, purpose_name: &str) {
        // Look up and remove the injected constraint IDs
        let injected_ids = match self.active_purposes.remove(purpose_name) {
            Some(ids) => ids,
            None => return, // Not active — no-op
        };

        // Update demand registry: remove demand for each ejected constraint node.
        for id in &injected_ids {
            self.demand.remove_demand(&NodeId::Constraint(id.clone()));
        }

        // Remove each injected constraint from the evaluation graph, then
        // rebuild the infrastructure so subsequent edit_param() calls no longer
        // route through purpose constraint dependencies.
        if let Some(state) = self.eval_state.as_mut() {
            for constraint_id in &injected_ids {
                state.snapshot.graph.constraints.remove(constraint_id);
            }
            state.reverse_index = ReverseDependencyIndex::build_from_graph(&state.snapshot.graph);
            state.trace_map = crate::deps::build_trace_map(&state.snapshot.graph);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Remove the objective if one was injected
        self.active_objective_map.remove(purpose_name);
    }

    /// Check whether a purpose is currently active.
    pub fn is_purpose_active(&self, purpose_name: &str) -> bool {
        self.active_purposes.contains_key(purpose_name)
    }

    /// Returns the currently active optimization objectives (injected by purposes).
    pub fn active_objectives(&self) -> Vec<&OptimizationObjective> {
        self.active_objective_map.values().collect()
    }
}
