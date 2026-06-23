//! Enforcement guard: all owner cites in the engine-seam G-allow allowlist
//! must remain non-terminal.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test engine_seam_g_allow_cites_live`
//!
//! Two tests:
//! - **Test A** (hermetic, always runs): in-memory DB seeded with current
//!   statuses. Real scanned cites → ZERO g-allow-orphaned. Synthetic done-
//!   cite control → exactly one g-allow-orphaned (test-has-teeth). Also
//!   guards that the scan actually found the expected cites.
//! - **Test B** (live anti-drift guard): open the real .taskmaster/tasks/
//!   tasks.db read-only; graceful-skip when absent (mirroring PTODO §6.7);
//!   assert ZERO g-allow-orphaned. Fires during `/audit` sweeps to catch
//!   real status drift.
//!
//! Scan scope: `// G-allow:` lines in the 7 source files pinned by
//! `engine_seam_orphans_g_allow.rs` PLUS the PINS array per-entry `//`
//! comment blocks (module doc excluded — contains only origin/provenance refs).

mod common;

use common::schema::{insert_task, seed_tasks_db};
use reify_audit::ptodo::{
    extract_g_allow_owner_cites, g_allow_marker_body, open_tasks_db,
    resolve_g_allow_owner_liveness, tasks_db_path,
};
use reify_audit::Severity;
use std::path::Path;

// -----------------------------------------------------------------------
// Workspace-relative paths of the 7 distinct source files pinned by PINS.
// -----------------------------------------------------------------------
const SOURCE_FILES: &[&str] = &[
    "crates/reify-eval/src/engine_build.rs",
    "crates/reify-kernel-gmsh/src/mesh_volume.rs",
    "crates/reify-mesh-morph/src/boundary.rs",
    "crates/reify-mesh-morph/src/elasticity.rs",
    "crates/reify-mesh-morph/src/laplacian.rs",
    "crates/reify-mesh-morph/src/lib.rs",
    "crates/reify-mesh-morph/src/quality.rs",
];

/// The engine_seam_orphans_g_allow.rs test file (source of the PINS array).
const PINS_FILE: &str = "crates/reify-audit/tests/engine_seam_orphans_g_allow.rs";

/// Find the workspace root from CARGO_MANIFEST_DIR (crates/reify-audit).
fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .to_path_buf()
}

/// Scan `// G-allow:` lines in a single source file, returning
/// `(rel_path, line_no, owner_cites, line_text)` tuples for every G-allow
/// marker line that has ≥1 owner cite.
fn scan_source_file(
    ws_root: &Path,
    rel_path: &str,
) -> Vec<(String, usize, Vec<u32>, String)> {
    let full_path = ws_root.join(rel_path);
    let content = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(_) => return vec![], // missing file — skip gracefully
    };
    let mut tuples = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;
        if let Some(body) = g_allow_marker_body(line) {
            let owners = extract_g_allow_owner_cites(body);
            if !owners.is_empty() {
                tuples.push((rel_path.to_string(), line_no, owners, line.to_string()));
            }
        }
    }
    tuples
}

/// Scan the PINS array per-entry `//` comment blocks from
/// `engine_seam_orphans_g_allow.rs`. Returns `(file_path, first_line_no,
/// owner_cites, joined_block)` for each block that yields ≥1 owner cite.
///
/// Excludes `//!` module-doc lines (which contain origin/provenance refs
/// like the terminal #3533 reference; including them would falsely fire
/// g-allow-orphaned on the current post-#4747 state).
///
/// Starts collecting comment blocks AFTER the `const PINS` declaration
/// line so module-level doc and `///` doc comments are skipped.
fn scan_pins_blocks(ws_root: &Path) -> Vec<(String, usize, Vec<u32>, String)> {
    let pins_path = ws_root.join(PINS_FILE);
    let content = match std::fs::read_to_string(&pins_path) {
        Ok(c) => c,
        Err(_) => return vec![], // missing file — skip gracefully
    };

    let mut tuples = Vec::new();
    let mut in_pins = false;
    let mut block: Vec<String> = Vec::new();
    let mut block_start_line: usize = 0;

    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = line.trim_start();

        if !in_pins {
            if trimmed.starts_with("const PINS") {
                in_pins = true;
            }
            continue;
        }

        // End of the PINS array.
        if trimmed.starts_with("];") {
            // Flush any trailing comment block (shouldn't happen, but safe).
            if !block.is_empty() {
                let joined = block.join(" ");
                let owners = extract_g_allow_owner_cites(&joined);
                if !owners.is_empty() {
                    tuples.push((PINS_FILE.to_string(), block_start_line, owners, joined));
                }
                block.clear();
            }
            break;
        }

        // Collect contiguous regular `//` comment lines (not `//!` or `///`).
        if trimmed.starts_with("//") && !trimmed.starts_with("//!") && !trimmed.starts_with("///") {
            let body = trimmed.strip_prefix("//").unwrap_or("").trim_start();
            if block.is_empty() {
                block_start_line = line_no;
            }
            block.push(body.to_string());
        } else if trimmed.starts_with('(') {
            // Found a tuple entry — emit the accumulated comment block.
            if !block.is_empty() {
                let joined = block.join(" ");
                let owners = extract_g_allow_owner_cites(&joined);
                if !owners.is_empty() {
                    tuples.push((PINS_FILE.to_string(), block_start_line, owners, joined));
                }
                block.clear();
            }
        } else if trimmed.is_empty()
            || trimmed.starts_with('"')
            || trimmed.starts_with(')')
            || trimmed.starts_with(',')
        {
            // Whitespace or tuple content — don't reset the comment accumulator.
        } else {
            // Any other non-comment, non-tuple line resets the accumulator.
            block.clear();
        }
    }

    tuples
}

// -----------------------------------------------------------------------
// Test A: hermetic, always runs — real markers + synthetic done-cite control.
// -----------------------------------------------------------------------

#[test]
fn engine_seam_g_allow_owner_cites_resolve_live_hermetic() {
    let ws_root = workspace_root();

    // Collect owner-cite tuples from (1) source files and (2) PINS blocks.
    let mut all_cites: Vec<(String, usize, Vec<u32>, String)> = Vec::new();
    for &rel_path in SOURCE_FILES {
        all_cites.extend(scan_source_file(&ws_root, rel_path));
    }
    all_cites.extend(scan_pins_blocks(&ws_root));

    // Guard: ensure the scan actually found expected owners; a rename/move
    // causing all files to be absent would make the "no orphaned" assertion
    // vacuously green.  At minimum one cite for each of #4743 and #4744 must
    // be present.
    let all_owner_ids: std::collections::HashSet<u32> = all_cites
        .iter()
        .flat_map(|(_, _, ids, _)| ids.iter().copied())
        .collect();
    assert!(
        all_owner_ids.contains(&4743),
        "expected at least one scanned owner cite for #4743; \
         if the source files moved, update SOURCE_FILES in this test. \
         Found owners: {all_owner_ids:?}"
    );
    assert!(
        all_owner_ids.contains(&4744),
        "expected at least one scanned owner cite for #4744; \
         if the source files moved, update SOURCE_FILES in this test. \
         Found owners: {all_owner_ids:?}"
    );
    assert!(
        !all_cites.is_empty(),
        "expected at least one owner-cite tuple from scan"
    );

    // Hermetic in-memory DB seeded with current known statuses.
    // 4743/4744 = pending (live owners).
    // 3429/2947 = cancelled, 2949 = done (provenance, exempt by grammar rules).
    // 9999 = done (synthetic control — must NOT appear in real scanned cites).
    let conn = seed_tasks_db();
    insert_task(&conn, "master", 4743, "pending");
    insert_task(&conn, "master", 4744, "pending");
    insert_task(&conn, "master", 3429, "cancelled");
    insert_task(&conn, "master", 2947, "cancelled");
    insert_task(&conn, "master", 2949, "done");
    insert_task(&conn, "master", 9999, "done"); // synthetic control only

    // (a) Real scanned cites must yield ZERO g-allow-orphaned findings.
    let real_findings = resolve_g_allow_owner_liveness(&conn, &all_cites)
        .expect("resolve_g_allow_owner_liveness");
    let orphaned: Vec<_> = real_findings
        .iter()
        .filter(|f| f.summary.starts_with("g-allow-orphaned:"))
        .collect();
    assert!(
        orphaned.is_empty(),
        "ZERO g-allow-orphaned expected for real engine-seam markers \
         (owners #4743/#4744 are pending); found {} orphaned:\n{}",
        orphaned.len(),
        orphaned
            .iter()
            .map(|f| format!("  {}: {}", f.task_id, f.summary))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // (b) Synthetic control: citing a `done` task must yield exactly one
    // g-allow-orphaned High finding (test-has-teeth).
    let control = vec![(
        "test-control".to_string(),
        1_usize,
        vec![9999_u32],
        "// G-allow: synthetic done-cite control for test-has-teeth".to_string(),
    )];
    let control_findings = resolve_g_allow_owner_liveness(&conn, &control)
        .expect("resolve control");
    let control_orphaned: Vec<_> = control_findings
        .iter()
        .filter(|f| f.summary.starts_with("g-allow-orphaned:"))
        .collect();
    assert_eq!(
        control_orphaned.len(),
        1,
        "synthetic done-cite control must yield exactly one g-allow-orphaned; \
         got {control_findings:?}"
    );
    assert_eq!(control_orphaned[0].severity, Severity::High);
    assert!(
        control_orphaned[0].summary.contains("#9999"),
        "control finding must name the id: {}",
        control_orphaned[0].summary
    );
}

// -----------------------------------------------------------------------
// Test B: live anti-drift guard — real tasks.db.
// -----------------------------------------------------------------------

#[test]
fn engine_seam_g_allow_owner_cites_resolve_live_real_db() {
    let ws_root = workspace_root();

    // Collect owner-cite tuples.
    let mut all_cites: Vec<(String, usize, Vec<u32>, String)> = Vec::new();
    for &rel_path in SOURCE_FILES {
        all_cites.extend(scan_source_file(&ws_root, rel_path));
    }
    all_cites.extend(scan_pins_blocks(&ws_root));

    if all_cites.is_empty() {
        eprintln!("engine_seam_g_allow_cites_live Test B: no cites scanned — skip");
        return;
    }

    // Open the real tasks.db read-only; graceful-skip when absent (worktree
    // without a local tasks.db — the live guard fires in the /audit sweep where
    // the DB is present in the main checkout).
    let db_path = tasks_db_path(&ws_root);
    let conn = match open_tasks_db(&db_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!(
                "engine_seam_g_allow_cites_live Test B: tasks.db absent at '{}' — skip \
                 (live guard fires in /audit sweep where DB is present)",
                db_path.display()
            );
            return;
        }
    };

    // Resolve real cites against the live DB; assert ZERO g-allow-orphaned.
    let findings = resolve_g_allow_owner_liveness(&conn, &all_cites)
        .expect("resolve_g_allow_owner_liveness against real DB");
    let orphaned: Vec<_> = findings
        .iter()
        .filter(|f| f.summary.starts_with("g-allow-orphaned:"))
        .collect();
    assert!(
        orphaned.is_empty(),
        "ZERO g-allow-orphaned expected for real engine-seam markers against \
         the live tasks.db. A cite has drifted to a terminal task. \
         Repoint the G-allow marker(s) and PINS entry to a live owner task.\n\
         {} orphaned finding(s):\n{}",
        orphaned.len(),
        orphaned
            .iter()
            .map(|f| format!("  {}: {}", f.task_id, f.summary))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
