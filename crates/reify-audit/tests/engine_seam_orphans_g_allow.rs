//! Pin: the 8 `pub fn` / `pub(crate) fn` that form the §3.2 engine-seam
//! producer-orphan subset swept under engine-integration-norm task ε (#3533).
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test engine_seam_orphans_g_allow`
//!
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! enforces presence of a `// G-allow:` marker on the line immediately above
//! each `pub fn`; this test asserts list membership (absent from `orphans[]`,
//! present exactly once in `allowed[]`).
//!
//! The specific task number and norm citation live only in the source
//! `// G-allow:` marker (single source of truth), so this test never carries
//! a second copy that could drift.  Neither assertion implies
//! `orphan_count == 0`; 400+ pre-existing baseline orphans in unrelated files
//! are intentionally not in scope here.
//!
//! Crates covered: reify-eval, reify-kernel-gmsh, reify-mesh-morph.
//!
//! **Removal contract**: each PINS entry is owned by the consumer task cited in
//! its source `// G-allow:` marker.  Once that task wires its consumer the
//! function gains a non-test caller, leaves `allowed[]`, and assertion (b)
//! fails.  The owning task MUST delete its row from `PINS` as part of the
//! consumer-wiring commit.  Delete this file entirely when all rows are removed.
//! The failure message lists all failing (file_suffix, fn_name) pairs — search
//! for them in this file when `PINS pin(s) failed` appears unexpectedly.
//!
//! **Wide-scope trade-off**: the audit runs at the wide `crates/reify-*/src`
//! scope (same as `new_orphans_2026_05_16_g_allow.rs`), so any name-token
//! occurrence of a pinned function name elsewhere in that scope — e.g. a local
//! `let eligible = ...` in another crate — can push `callers > 0`, silently
//! removing the function from `allowed[]` and tripping assertion (b) without a
//! real consumer being wired.  The `eligible` pin
//! (crates/reify-mesh-morph/src/lib.rs) carries the highest collision risk due
//! to the word's frequency in English.  Before deleting any PINS row after an
//! assertion-(b) failure, confirm a real call edge was wired:
//! `rg '\bFN_NAME\b' crates/reify-*/src`.
//!
//! Graceful skip: if `python3`, `git`, or the audit script are absent from
//! PATH/disk the test prints a note to stderr and returns without failing.
//! The shared helper is `reify_test_support::run_orphan_audit`.

use reify_test_support::run_orphan_audit;

/// (file_suffix, fn_name)
///
/// `file_suffix` is the suffix of the `file` field in the JSON output
/// (repo-relative path from the workspace root).  Two rows share the same
/// `elasticity.rs` suffix but have distinct `fn_name` values, so each matches
/// exactly once in `allowed[]`.
const PINS: &[(&str, &str)] = &[
    // §3.2 realization-kind dispatch seam (VolumeMesh); consumer task #3429
    // (CN-contract §8 task κ — adds execute_realization_ops call edge) / #2947.
    (
        "crates/reify-eval/src/engine_build.rs",
        "dispatch_volume_mesh",
    ),
    // §3.2 Gmsh tet-mesher producer; consumer task #3429 / #2947.
    (
        "crates/reify-kernel-gmsh/src/mesh_volume.rs",
        "mesh_surface_to_volume_with_diagnostics",
    ),
    // reify-mesh-morph public API — §3.2 realization-kind dispatch producers;
    // consumer task #2947 (mesh-morph VolumeMesh realization wiring) / #3429.
    (
        "crates/reify-mesh-morph/src/boundary.rs",
        "compute_dirichlet_bcs",
    ),
    (
        "crates/reify-mesh-morph/src/elasticity.rs",
        "elasticity_morph_with_cg_opts",
    ),
    (
        "crates/reify-mesh-morph/src/elasticity.rs",
        "elasticity_morph",
    ),
    (
        "crates/reify-mesh-morph/src/laplacian.rs",
        "laplacian_smooth",
    ),
    // NOTE: `eligible` is a common English word; at the wide `crates/reify-*/src`
    // scope a future `let eligible = ...` in any crate could create a name-token
    // collision (callers > 0 → exits allowed[]).  See module-doc "Wide-scope
    // trade-off" before deleting this row after an assertion-(b) failure.
    (
        "crates/reify-mesh-morph/src/lib.rs",
        "eligible",
    ),
    (
        "crates/reify-mesh-morph/src/quality.rs",
        "quality_check",
    ),
];

#[test]
fn engine_seam_orphans_are_g_allow_marked() {
    let Some(result) = run_orphan_audit("crates/reify-*/src") else {
        return;
    };

    let mut failures: Vec<String> = Vec::new();

    for &(file_suffix, fn_name) in PINS {
        // (a) Must NOT appear in orphans[] for the given file.
        let in_orphans = result["orphans"]
            .as_array()
            .expect("orphans must be an array")
            .iter()
            .any(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(file_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            });

        if in_orphans {
            failures.push(format!(
                "  FAIL (a) `{fn_name}` ({file_suffix}): still listed as an orphan \
                 — the `// G-allow:` marker may be missing or misplaced (must be \
                 on the line immediately above `pub fn`, with no blank line)."
            ));
        }

        // (b) Must appear EXACTLY ONCE in allowed[] for the given file.
        let matching_allowed: Vec<_> = result["allowed"]
            .as_array()
            .expect("allowed must be an array")
            .iter()
            .filter(|entry| {
                entry["file"]
                    .as_str()
                    .map(|f| f.ends_with(file_suffix))
                    .unwrap_or(false)
                    && entry["name"].as_str() == Some(fn_name)
            })
            .collect();

        if matching_allowed.len() != 1 {
            let n = matching_allowed.len();
            let detail = if n == 0 {
                format!(
                    "0 entries — either a real consumer call was wired (delete \
                     this PINS row) OR a same-name token elsewhere in \
                     `crates/reify-*/src` pushed callers > 0 (incidental \
                     collision); run `rg '\\b{fn_name}\\b' crates/reify-*/src` \
                     to distinguish before removing the row."
                )
            } else {
                format!("{n} entries — unexpected duplicate `// G-allow:` markers")
            };
            failures.push(format!(
                "  FAIL (b) `{fn_name}` ({file_suffix}): expected exactly 1 \
                 entry in allowed[]; {detail}"
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} PINS pin(s) failed:\n{}\n\n\
             PINS: `crates/reify-audit/tests/engine_seam_orphans_g_allow.rs`\n\
             Full orphans list:\n{:#}\n\
             Full allowed list:\n{:#}",
            failures.len(),
            failures.join("\n"),
            result["orphans"],
            result["allowed"]
        );
    }
}
