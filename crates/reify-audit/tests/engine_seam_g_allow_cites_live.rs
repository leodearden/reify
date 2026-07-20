//! Enforcement guard for the §3.2 engine-seam G-allow allowlist.
//!
//! **Seam status (task #5255): fully graduated.** All three seam owners —
//! #4743 (α, VolumeMesh realization), #4744 (β, morph arm), and #5007
//! (quality hardening) — are now terminal, and the morph seam has no live
//! successor. Every remaining engine-seam `// G-allow:` cite is therefore
//! provenance-exempt (`(done)` / `re-homed` / `formerly`), so the live scan
//! extracts ZERO owner cites. The repo-wide hard gate
//! (`g_allow_repo_wide_hard_gate_live`) is now the drift backstop for any
//! future terminal-cite regression in these files; this test's has-teeth is
//! carried structurally (marker-line presence + a synthetic control) rather
//! than by a live-owner cite, because no live owner remains to point at.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test engine_seam_g_allow_cites_live`
//!
//! Two tests:
//! - **Test A** (hermetic, always runs): in-memory DB seeded with the REAL
//!   terminal statuses of the (now-graduated) seam owners. Real scanned cites
//!   → ZERO g-allow-orphaned (post-graduation the scan is empty; a residual
//!   bare terminal cite would resolve as orphaned and fail). Test-has-teeth is
//!   carried by (1) a marker-line-presence guard over the two still-orphan
//!   pinned functions (a file move/rename/deletion still fails — anti-vacuous-
//!   green) and (2) a synthetic `done`-cite control that must yield exactly one
//!   g-allow-orphaned.
//! - **Test B** (live anti-drift guard): open the real .taskmaster/tasks/
//!   tasks.db read-only; graceful-skip when absent (mirroring PTODO §6.7) OR
//!   when the scan yields no cites (the graduated steady state); assert ZERO
//!   g-allow-orphaned when cites are present. Fires during `/audit` sweeps to
//!   catch real status drift.
//!
//! Scan scope: `// G-allow:` lines in the source files pinned by
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
// Workspace-relative paths of the distinct source files pinned by PINS.
//
// Task #4743 (α) removed the two §3.2 VolumeMesh-seam G-allow markers when it
// wired their real consumers — `dispatch_volume_mesh`
// (crates/reify-eval/src/engine_build.rs) and `mesh_surface_to_volume_with_diagnostics`
// (crates/reify-kernel-gmsh/src/mesh_volume.rs) — so those two files no longer
// carry a `// G-allow:` cite and drop out of this scan scope. The remaining
// entries were the #4744 (β) morph-arm producers in reify-mesh-morph; task
// #5255 graduated #4744 too (all seam owners terminal), deleting the now-wired
// markers and done-annotating the two still-orphan ones
// (elasticity_morph_with_cg_opts, eligible), so every cite these files now
// carry is provenance-exempt and the scan yields no owner.
// -----------------------------------------------------------------------
const SOURCE_FILES: &[&str] = &[
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

/// Anti-vacuous-green / anti-file-move guard for the graduated seam.
///
/// Post-graduation every engine-seam owner cite is provenance-exempt, so the
/// live scan extracts ZERO owners and the "zero orphaned" assertion can no
/// longer prove the scan actually ran over real markers. This structural check
/// asserts that a STILL-ORPHAN marker is physically present within the leading
/// comment/attribute block above `pub fn <fn_name>(` in `rel_path` — scanning
/// upward over contiguous `//` and `#[…]` lines, so inserting an attribute
/// (`#[inline]`, `#[cfg(…)]`, `#[allow(…)]`) or a doc line between the marker
/// and the signature does not spuriously fail; the invariant is only that the
/// marker is present somewhere in that block. A file move/rename/deletion that
/// drops the marker fails here — the same event would also un-pin the
/// orphan-producer audit in `engine_seam_orphans_g_allow.rs`.
fn assert_g_allow_marker_above_fn(ws_root: &Path, rel_path: &str, fn_name: &str) {
    let full = ws_root.join(rel_path);
    let content = std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("read {rel_path} for still-orphan marker guard: {e}"));
    let lines: Vec<&str> = content.lines().collect();
    let sig = format!("pub fn {fn_name}(");
    let fn_idx = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&sig))
        .unwrap_or_else(|| {
            panic!(
                "expected `pub fn {fn_name}(` in {rel_path} (still-orphan marker guard); \
                 a move/rename/deletion would remove it — if the seam legitimately \
                 changed, update SOURCE_FILES/PINS and this guard together"
            )
        });
    // Scan upward over the fn's contiguous leading block — comment lines (`//`,
    // `///`, `//!`) and attribute lines (`#[…]`) — for the `// G-allow:` marker.
    // Requiring exact adjacency (`lines[fn_idx - 1]`) would spuriously fail if a
    // future edit inserted an attribute or a doc line between the marker and the
    // signature; the invariant is only that the marker sits somewhere in the
    // leading block, not on the immediately-preceding line.
    let mut marker_ok = false;
    let mut idx = fn_idx;
    while idx > 0 {
        idx -= 1;
        let trimmed = lines[idx].trim_start();
        let in_leading_block =
            trimmed.starts_with("//") || trimmed.starts_with("#[") || trimmed.starts_with("#![");
        if !in_leading_block {
            break; // left the fn's contiguous leading comment/attribute block
        }
        if g_allow_marker_body(lines[idx]).is_some() {
            marker_ok = true;
            break;
        }
    }
    assert!(
        marker_ok,
        "expected a `// G-allow:` marker within the leading comment/attribute \
         block above `pub fn {fn_name}(` in {rel_path} (fn at line {}); the \
         still-orphan marker must remain to suppress the orphan-producer audit.",
        fn_idx + 1,
    );
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

    // Anti-vacuous-green / anti-file-move guard. Post-graduation (task #5255)
    // every engine-seam owner cite is provenance-exempt, so `all_cites` is empty
    // in the steady state and the "zero orphaned" assertion below can no longer
    // prove the scan ran. Instead assert the two STILL-ORPHAN markers are
    // physically present on the line immediately above their `pub fn`: a file
    // move/rename/deletion that drops these markers still fails (and would also
    // un-pin the orphan-producer audit in engine_seam_orphans_g_allow.rs).
    //
    // A live-owner-cite guard (formerly `all_owner_ids.contains(&4744)`) is no
    // longer possible: `extract_g_allow_owner_cites` skips exempt cites, so a
    // still-extractable cite and a non-orphaned cite are mutually exclusive once
    // the owner is terminal — and all three seam owners (#4743/#4744/#5007) are
    // terminal with no live successor to repoint to. Precedent: feat(4743)
    // 0487726b18 dropped #4743 from this same guard when it graduated.
    assert_g_allow_marker_above_fn(
        &ws_root,
        "crates/reify-mesh-morph/src/elasticity.rs",
        "elasticity_morph_with_cg_opts",
    );
    assert_g_allow_marker_above_fn(&ws_root, "crates/reify-mesh-morph/src/lib.rs", "eligible");

    // Hermetic in-memory DB seeded with the REAL terminal statuses of the
    // now-graduated seam owners (task #5255): #4744 (β morph arm), #4743 (α),
    // and #5007 (quality) are all done; 3429/2947 = cancelled; 2949 = done
    // (debug-RPC provenance). Every scanned cite is provenance-exempt, so in the
    // graduated steady state no owner resolves — but a residual bare terminal
    // cite (e.g. a regression re-adding `#4744` without a `(done)` annotation)
    // WOULD resolve as orphaned and fail assertion (a) below.
    // 9999 = done (synthetic control — must NOT appear in real scanned cites).
    let conn = seed_tasks_db();
    insert_task(&conn, "master", 4744, "done");
    insert_task(&conn, "master", 4743, "done");
    insert_task(&conn, "master", 5007, "done");
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
        "ZERO g-allow-orphaned expected for the graduated engine-seam markers \
         (all owners terminal → every cite provenance-exempt → empty scan). A \
         non-empty result means a residual BARE terminal cite regressed into a \
         SOURCE_FILES marker or a PINS comment block — annotate it `(done)` or \
         re-home it. Found {} orphaned:\n{}",
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

/// Live anti-drift guard: resolve the real scanned engine-seam cites against
/// the real `.taskmaster/tasks/tasks.db` and assert ZERO g-allow-orphaned.
///
/// Post-graduation (task #5255) every engine-seam cite is provenance-exempt, so
/// the scan yields no cites and this test graceful-skips — the repo-wide
/// `g_allow_repo_wide_hard_gate_live` is the live drift backstop in this steady
/// state. The guard still has teeth: a future regression that re-adds a bare
/// terminal owner cite to a SOURCE_FILES marker or a PINS comment block makes
/// `all_cites` non-empty again, and — if that owner is terminal in the live DB
/// — fails here. Also graceful-skips when the DB is absent (task worktrees,
/// mirroring PTODO §6.7); the live guard fires in the `/audit` sweep where the
/// main-checkout DB is present.
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
        eprintln!(
            "engine_seam_g_allow_cites_live Test B: no engine-seam cites scanned \
             (graduated steady state — all owners terminal, cites provenance-exempt); \
             repo-wide g_allow_repo_wide_hard_gate_live is the live drift backstop — skip"
        );
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
