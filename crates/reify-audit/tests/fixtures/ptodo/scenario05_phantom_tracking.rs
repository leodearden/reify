// Scenario 5 (PRD §9): phantom-tracking prose — two claims of tracking.
// The first names no task, so it is a structural phantom-tracking finding.
// The second backs the same claim with a canonical cite, so it is discharged:
// structurally silent (α emits no finding), while the β liveness lane resolves
// the cite against the task DB and flags it `orphaned` once the cited task is
// terminal. That status is supplied only by the test-seeded DB, never the real
// tasks.db.
// tracked as a follow-up task
// tracked separately as #5555
fn scenario05() {}
