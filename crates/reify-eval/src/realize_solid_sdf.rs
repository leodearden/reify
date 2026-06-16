// PRD §7.1 γ: realize_solid_sdf — BRep→Mesh→Voxel→SampledField post-build recipe.
//
// Turns an already-realized BRep solid into a CPU-resident queryable SDF by
// demanding a Voxel realization and driving β's BRep→Mesh→Voxel chain, then
// densifying via α.  Returns `None` on every degradation path (D5: the caller ζ
// maps `None` → self-describing `Undef` + diagnostic + `Indeterminate`, never a
// fabricated number).
//
// PRD §4 D1 — post-build direct recipe: γ does NOT re-enter the dispatcher BFS
// / realization loop and does NOT modify `demanded_reprs_for_template`.  The
// subject is already realized; γ runs the same recipe β's executor runs
// (engine_build.rs:4899-4970) directly.

impl crate::Engine {
    // Method lands here in step-2 / step-4.
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use reify_core::RealizationNodeId;
    use reify_ir::{GeometryHandleId, GeometryHandleRef, SampledField, SampledGridKind};
    use reify_test_support::mocks::MockConstraintChecker;

    use crate::Engine;

    fn make_engine() -> Engine {
        Engine::new(Box::new(MockConstraintChecker::new()), None)
    }
}
