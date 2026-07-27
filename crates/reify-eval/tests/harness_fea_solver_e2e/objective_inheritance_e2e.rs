// Integration tests for the F-inherit ζ regression corpus and CI example
// (task #4826).
//
// ## Purpose
//
// 1. **D1 back-compat byte-identity corpus** (INV-2 / G6 branch-3, BT1/BT2/BT7):
//    - BT1 – single scope with an explicit objective (minimize → solver clamp).
//    - BT2 – two uncoupled scopes in both declaration orders (AB and BA);
//      asserts stdout is byte-identical regardless of order (INV-2 proof).
//    - BT7 – objective-less scope; synthetic-centrality midpoint.
//
// 2. **D2 BT5 CI-example** (`examples/objective_inheritance.ri`):
//    - `reify eval` resolves C.k to a non-undef concrete value under
//      inherited governance.
//    - `reify explain` shows C.k `inherited from P` and P.w `source=explicit`.
//
// ## Harness notes
//
// The test binary lives at `.../target/<profile>/deps/<testbin>`.  Its
// grandparent is `.../target/<profile>`, where the `reify` CLI binary lives.
// During the merge gate's RELEASE pass, `reify-cli` is NOT rebuilt (it is not
// release-sensitive); the debug-profile bin built in the preceding debug pass is
// used as the fallback.

/// Resolve the pre-built `reify` binary.
///
/// Prefers the profile-local binary next to the test binary; falls back to
/// `target/debug/reify` (used by the merge gate's release pass, where the reify
/// CLI is NOT rebuilt).
fn resolve_reify_bin() -> std::path::PathBuf {
    let test_bin = std::env::current_exe().expect("current_exe");
    let profile_dir = test_bin
        .parent()
        .and_then(|p| p.parent())
        .expect("test binary lives in target/<profile>/deps");
    let profile_local = profile_dir.join("reify");
    if profile_local.exists() {
        profile_local
    } else {
        // Merge gate release pass: reify-cli not in this scope — fall back to
        // the debug-profile bin built by the earlier debug pass.
        profile_dir
            .parent()
            .map(|target| target.join("debug").join("reify"))
            .filter(|p| p.exists())
            .unwrap_or(profile_local)
    }
}

/// Resolve the common test paths: the crate manifest dir, workspace root,
/// and the pre-built `reify` binary.
///
/// Eliminates the identical 4-line boilerplate from every test function; a
/// drift in the `nth(2)` ancestor depth would otherwise silently desync
/// across copies.
fn resolve_test_paths() -> (&'static str, std::path::PathBuf, std::path::PathBuf) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root two levels above crates/reify-eval")
        .to_path_buf();
    let reify_bin = resolve_reify_bin();
    (manifest, workspace_root, reify_bin)
}

/// Run `reify eval <fixture>` from the workspace root, return (exit_status, stdout, stderr).
fn run_eval(
    reify_bin: &std::path::Path,
    workspace_root: &std::path::Path,
    fixture: &std::path::Path,
) -> (bool, String, String) {
    let out = std::process::Command::new(reify_bin)
        .current_dir(workspace_root)
        .arg("eval")
        .arg(fixture)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn pre-built reify binary at {}: {e}; \
                 is it built? Run `cargo test -p reify-cli` or the full \
                 merge gate debug pass that builds all [[bin]] targets.",
                reify_bin.display()
            )
        });
    let success = out.status.success();
    let stdout = String::from_utf8(out.stdout).expect("stdout must be valid UTF-8");
    let stderr = String::from_utf8(out.stderr).expect("stderr must be valid UTF-8");
    (success, stdout, stderr)
}

/// Assert that `stdout` matches the on-disk golden, regenerating when
/// `REIFY_REGENERATE_GOLDEN=1` is set.  Returns `true` when regeneration
/// happened (test should return immediately without further assertions).
fn assert_or_regen_golden(
    stdout: &str,
    golden_path: &std::path::Path,
    golden_label: &str,
) -> bool {
    if std::env::var("REIFY_REGENERATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(
            golden_path
                .parent()
                .expect("golden must be inside a directory"),
        )
        .expect("create golden parent dir");
        std::fs::write(golden_path, stdout)
            .unwrap_or_else(|e| panic!("failed to write golden {golden_label}: {e}"));
        return true;
    }
    let expected = std::fs::read_to_string(golden_path).unwrap_or_else(|_| {
        panic!(
            "golden {golden_label} missing at {}; \
             run once with REIFY_REGENERATE_GOLDEN=1 to generate",
            golden_path.display()
        )
    });
    assert_eq!(
        stdout, expected,
        "`reify eval` stdout drifted from golden {golden_label}; \
         re-run with REIFY_REGENERATE_GOLDEN=1 to update",
    );
    false
}

// ── D1 corpus: back-compat byte-identity (BT1 / BT2 / BT7) ──────────────────

/// BT1 – single scope with an explicit `minimize` objective.
///
/// Fixture: `tests/fixtures/backcompat/bt1_single_scope.ri`
/// Golden:  `tests/golden/bt1_single_scope.txt`
///
#[test]
fn bt1_single_scope_byte_identity() {
    let (manifest, workspace_root, reify_bin) = resolve_test_paths();
    let fixture = std::path::Path::new(manifest)
        .join("tests/fixtures/backcompat/bt1_single_scope.ri");
    let golden = std::path::Path::new(manifest)
        .join("tests/golden/bt1_single_scope.txt");

    let (success, stdout, stderr) = run_eval(&reify_bin, &workspace_root, &fixture);
    assert!(
        success,
        "`reify eval` exited non-zero for BT1;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert_or_regen_golden(&stdout, &golden, "bt1_single_scope.txt");
}

/// BT7 – objective-less scope; synthetic-centrality midpoint.
///
/// Fixture: `tests/fixtures/backcompat/bt7_objectiveless_centrality.ri`
/// Golden:  `tests/golden/bt7_objectiveless_centrality.txt`
///
#[test]
fn bt7_objectiveless_centrality_byte_identity() {
    let (manifest, workspace_root, reify_bin) = resolve_test_paths();
    let fixture = std::path::Path::new(manifest)
        .join("tests/fixtures/backcompat/bt7_objectiveless_centrality.ri");
    let golden = std::path::Path::new(manifest)
        .join("tests/golden/bt7_objectiveless_centrality.txt");

    let (success, stdout, stderr) = run_eval(&reify_bin, &workspace_root, &fixture);
    assert!(
        success,
        "`reify eval` exited non-zero for BT7;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert_or_regen_golden(&stdout, &golden, "bt7_objectiveless_centrality.txt");
}

/// BT2 – two uncoupled scopes; declaration order independence (INV-2).
///
/// Runs both `bt2_uncoupled_ab.ri` (A declared first) and `bt2_uncoupled_ba.ri`
/// (B declared first) and asserts:
///   (a) both match the shared golden `bt2_uncoupled.txt`, AND
///   (b) the two orderings produce BYTE-IDENTICAL stdout (direct INV-2 proof).
///
#[test]
fn bt2_uncoupled_declaration_order_independence() {
    let (manifest, workspace_root, reify_bin) = resolve_test_paths();

    let fixture_ab = std::path::Path::new(manifest)
        .join("tests/fixtures/backcompat/bt2_uncoupled_ab.ri");
    let fixture_ba = std::path::Path::new(manifest)
        .join("tests/fixtures/backcompat/bt2_uncoupled_ba.ri");
    let golden = std::path::Path::new(manifest)
        .join("tests/golden/bt2_uncoupled.txt");

    let (success_ab, stdout_ab, stderr_ab) = run_eval(&reify_bin, &workspace_root, &fixture_ab);
    assert!(
        success_ab,
        "`reify eval` exited non-zero for BT2-AB;\nstdout:\n{stdout_ab}\nstderr:\n{stderr_ab}"
    );

    let (success_ba, stdout_ba, stderr_ba) = run_eval(&reify_bin, &workspace_root, &fixture_ba);
    assert!(
        success_ba,
        "`reify eval` exited non-zero for BT2-BA;\nstdout:\n{stdout_ba}\nstderr:\n{stderr_ba}"
    );

    // (b) Direct INV-2 proof: both orderings produce byte-identical output.
    assert_eq!(
        stdout_ab, stdout_ba,
        "BT2 INV-2 violation: bt2_uncoupled_ab.ri and bt2_uncoupled_ba.ri \
         produce DIFFERENT stdout — declaration order leaked into eval output"
    );

    // (a) Both match the shared golden (regen writes from AB; BA identity proved above).
    if assert_or_regen_golden(&stdout_ab, &golden, "bt2_uncoupled.txt") {
        return;
    }
    // If not regenerating, also verify BA matches the golden.
    let expected = std::fs::read_to_string(&golden).expect("golden bt2_uncoupled.txt");
    assert_eq!(
        stdout_ba, expected,
        "`reify eval bt2_uncoupled_ba.ri` drifted from bt2_uncoupled.txt"
    );
}

// ── D2: BT5 CI-example (examples/objective_inheritance.ri) ───────────────────

/// BT5 eval – child C resolves under inherited governance (non-undef).
///
/// Asserts exit 0 and that stdout contains `C.k =` but NOT `C.k = undef`.
/// NEVER asserts a specific numeric optimum (§3.2 honesty boundary).
///
#[test]
fn bt5_example_eval_resolves_under_inherited_governance() {
    let (_, workspace_root, reify_bin) = resolve_test_paths();
    let example = workspace_root.join("examples/objective_inheritance.ri");

    let (success, stdout, stderr) = run_eval(&reify_bin, &workspace_root, &example);
    assert!(
        success,
        "`reify eval examples/objective_inheritance.ri` exited non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // C.k must resolve to a concrete value (not undef).
    assert!(
        stdout.contains("C.k ="),
        "expected 'C.k =' in stdout but got:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("C.k = undef"),
        "C.k must NOT be undef — it should resolve under inherited governance;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// BT5 explain – C.k carries inherited_from=P provenance; P.w is source=explicit.
///
/// Asserts:
///   (a) exit 0
///   (b) C.k line contains "inherited from P" (source=inherited + clause)
///   (c) C.k line does NOT contain "synthetic-centrality" (centrality suppressed)
///   (d) P.w line contains "source=explicit" and NOT "inherited from"
///   (e) Two consecutive runs produce byte-identical stdout (determinism)
///
#[test]
fn bt5_example_explain_shows_inherited_provenance() {
    let (_, workspace_root, reify_bin) = resolve_test_paths();
    let example = workspace_root.join("examples/objective_inheritance.ri");

    let explain_output = |label: &str| -> (bool, String, String) {
        let out = std::process::Command::new(&reify_bin)
            .current_dir(&workspace_root)
            .arg("explain")
            .arg(&example)
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "failed to spawn reify at {} for explain ({label}): {e}",
                    reify_bin.display()
                )
            });
        let success = out.status.success();
        let stdout = String::from_utf8(out.stdout).expect("stdout UTF-8");
        let stderr = String::from_utf8(out.stderr).expect("stderr UTF-8");
        (success, stdout, stderr)
    };

    let (success, stdout, stderr) = explain_output("run 1");

    // (a) exit 0
    assert!(
        success,
        "`reify explain examples/objective_inheritance.ri` exited non-zero;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // (b) C.k line must contain "inherited from P"
    let ck_line = stdout
        .lines()
        .find(|l| l.contains("C.k"))
        .unwrap_or_else(|| panic!("no C.k line in stdout:\n{stdout}\nstderr:\n{stderr}"));
    assert!(
        ck_line.contains("inherited from P"),
        "C.k line must contain 'inherited from P';\nline: {ck_line:?}\nstdout:\n{stdout}"
    );

    // (c) C.k must NOT show synthetic-centrality (inheritance suppresses it)
    assert!(
        !ck_line.contains("synthetic-centrality"),
        "C.k line must NOT contain 'synthetic-centrality' — inheritance suppresses centrality;\nline: {ck_line:?}\nstdout:\n{stdout}"
    );

    // (d) P.w line must contain "source=explicit" and must NOT contain "inherited from"
    let pw_line = stdout
        .lines()
        .find(|l| l.contains("P.w"))
        .unwrap_or_else(|| panic!("no P.w line in stdout:\n{stdout}\nstderr:\n{stderr}"));
    assert!(
        pw_line.contains("source=explicit"),
        "P.w line must contain 'source=explicit';\nline: {pw_line:?}\nstdout:\n{stdout}"
    );
    assert!(
        !pw_line.contains("inherited from"),
        "P.w line must NOT contain 'inherited from';\nline: {pw_line:?}\nstdout:\n{stdout}"
    );

    // (e) Determinism: second run produces byte-identical stdout
    let (_, stdout2, _) = explain_output("run 2");
    assert_eq!(
        stdout, stdout2,
        "`reify explain` output is not deterministic (runs 1 and 2 differ)"
    );
}
