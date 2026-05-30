//! Pin: the 8 `pub fn` / `pub(crate) fn` that form the §3.2 engine-seam
//! producer-orphan subset swept under engine-integration-norm task ε (#3533).
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test engine_seam_orphans_g_allow`
//!
//! Each pinned function must carry a `// G-allow:` marker citing
//! `engine-integration-norm §3.2` and a pending consumer task number.
//! The orphan-producer audit script (`scripts/audit-orphan-producers.sh`)
//! enforces presence of a `// G-allow:` marker on the line immediately above
//! each `pub fn`; this test additionally asserts list membership (absent from
//! `orphans[]`, present in `allowed[]`) and that the reason string:
//!   (a) cites *some* tracked owner task as `#NNNN`, and
//!   (b) contains the substring `engine-integration-norm`.
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
//! below will fail with "found 0 entries".  The owning task MUST delete its
//! row from `PINS` as part of the consumer-wiring commit.  Delete this file
//! entirely when all rows are removed.  The failure message includes the fn
//! name — search for it in this file when
//! `assert_eq!(matching_allowed.len(), 1)` fires unexpectedly.
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

        assert!(
            !in_orphans,
            "`{fn_name}` in {file_suffix} is still listed as an orphan — \
             the `// G-allow:` marker may be missing or misplaced (must be \
             on the line immediately above `pub fn`, with no blank line).\n\
             Full orphans list:\n{:#}",
            result["orphans"]
        );

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

        assert_eq!(
            matching_allowed.len(),
            1,
            "`{fn_name}` in {file_suffix} must appear exactly once in \
             allowed[]; found {} entries.  If you just wired a consumer \
             for `{fn_name}`, delete its row from \
             PINS in `crates/reify-audit/tests/engine_seam_orphans_g_allow.rs`.\n\
             Full allowed list:\n{:#}",
            matching_allowed.len(),
            result["allowed"]
        );

        let reason = matching_allowed[0]["allow_reason"].as_str().unwrap_or_default();

        // (c) The allow_reason must cite SOME tracked owner task as `#NNNN`.
        let bytes = reason.as_bytes();
        let cites_task = bytes.iter().enumerate().any(|(i, &b)| {
            b == b'#' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
        });
        assert!(
            cites_task,
            "`{fn_name}` allow_reason must cite a tracked task as `#NNNN`; got: {reason:?}"
        );

        // (d) The allow_reason must also cite engine-integration-norm, confirming
        // this marker was added under task ε rather than an unrelated sweep.
        assert!(
            reason.contains("engine-integration-norm"),
            "`{fn_name}` allow_reason must contain \"engine-integration-norm\"; \
             got: {reason:?}"
        );
    }
}
