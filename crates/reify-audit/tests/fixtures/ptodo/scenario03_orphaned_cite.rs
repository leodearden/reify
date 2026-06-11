// Scenario 3 (PRD §9): an orphaned citation — a canonical `#NNNN` comment
// marker whose cited task has reached a terminal status (done / cancelled).
// Structurally this is "tracked" (α emits no finding); the β liveness lane
// resolves the cite against the task DB and flags it `orphaned`. The terminal
// status is supplied only by the test-seeded DB, never the real tasks.db.
// TODO(#4444): wire the orphaned-cite path
fn scenario03() {}
