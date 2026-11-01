// Scenario 21 (PRD §8.1, lane δ-C): prose promises that name a task as the
// OWNER of work still to be done. Shaped after the `structure def Part` block
// crates/reify-compiler/stdlib/modal_analysis.ri carried before its promise
// prose was rewritten, recast into the owner forms δ-C recognises.
//
// Seed contract (tests/ptodo.rs, tests/infra/test_reify_audit_ptodo.sh): the
// promised owner is seeded `done`, so each of the FOUR owner lines below — one
// per rule, plus the legacy `task N` spelling — becomes one High `orphaned`
// finding. The live owner is seeded non-terminal and stays silent, and so do
// the three provenance mentions of the promised owner's own id: δ-C reads a
// cite's syntactic role, not the mere presence of a terminal id on the line.
//
// No line carries a structural marker, nor δ-B's deferral prose beside a
// canonical cite, so with no task DB the fixture contributes nothing and
// tests/cli.rs's exact fixture-tree count is unchanged.
//
// Self-match safety: this path sits under the allowlisted `crates/reify-audit/`
// prefix, so the live sweep never sees it. Copied into a temp project root by
// tests/ptodo.rs and tests/cli.rs, its path is NOT allowlisted and it IS swept.

// --- provenance: the promised owner's id, mentioned but never owning -------

// `Part` is a minimal opaque marker declared in this module (task 4578, per
// the stdlib surface-type substrate PRD).
// The opaque form was introduced by #4578 so the three `part : Part` params
// resolve to one nominal type.
// `Part` was a placeholder until #4578 renamed it into a structure.

// --- owner position: one line per rule, all naming the promised owner -----

// Growth of `Part` into a full geometry+material+topology structure is owned by #4578.
// The material fields are deferred to task 4578 and the registry it introduces.
// Task 4578 will grow `Part` into a full structure.
// `Part` stays opaque until #4578 lands the geometry+material+topology fields.

// --- owner position, live owner: silent while that task is open -----------

// The GeometryHandleRef join-key convergence is owned by #7097.
pub struct Part;
