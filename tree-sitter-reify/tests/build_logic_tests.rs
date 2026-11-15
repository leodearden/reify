//! Tests for build.rs pure logic functions.
//!
//! Since build.rs is compiled as a standalone build script by cargo, its
//! functions cannot be `use`d by a test target. The historical workaround was
//! to HAND-COPY them here — and that is precisely how `#6992` shipped: a green
//! 1879-line suite exercising a private replica while the real build script
//! carried the defect.
//!
//! So the staleness primitives now live in ONE file, `../build_support.rs`,
//! and both `build.rs` and this test target `include!` it. A test that passes
//! here is a statement about the code cargo actually runs.
//!
//! Every build.rs function this file exercises comes from that shared file, so
//! redefining one here is a duplicate-definition compile error, not a silent
//! replica.
//!
//! DO NOT ASSERT ON build.rs's OR build_support.rs's SOURCE TEXT HERE. A
//! `contains("write_generation_stamps(")`-style scan is satisfied by a comment or a
//! commented-out line — delete the real call, keep its explanation above it,
//! and the test stays green — and its negative form pins one SPELLING, so
//! `rel.ends_with("parser.c")` reintroduces a defect that
//! `!contains("rel == \"src/parser.c\"")` was watching for.
//!
//! Routing such a scan through the comment-stripper this file already owns
//! (`find_bare_build_rs_violations`) was considered and REJECTED: it closes the
//! comment-satisfiability half and leaves the spelling-variance half — the
//! deeper of the two — exactly as open, while making the result look guarded.
//! Contracts about what build.rs DOES belong in
//! `tests/infra/test_tree_sitter_pipeline.sh`, whose subjects are runtime
//! values: the directives cargo actually captured in
//! `target/*/build/tree-sitter-reify-*/output`, and the stamp bytes on disk
//! after a real `cargo build` (`#6992` review). Source scans of THIS file's own
//! text are a different thing and stay — they pin this file's conventions.
include!("../build_support.rs");

use std::path::Path;

/// Absolute path to this test file, resolved at compile time via CARGO_MANIFEST_DIR.
/// Used by source-level regression tests that read this file's own contents.
const THIS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/build_logic_tests.rs");

/// Absolute path to build.rs, resolved at compile time via CARGO_MANIFEST_DIR.
/// Used by source-level regression tests that read the build script's contents.
const BUILD_RS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs");

/// Reads this test file's own source code and returns it as a `String`.
///
/// Wraps `std::fs::read_to_string(THIS_FILE)` so callers don't repeat the
/// expect message. All self-inspection tests should use this helper rather
/// than calling `read_to_string(THIS_FILE)` directly.
fn read_self_source() -> String {
    std::fs::read_to_string(THIS_FILE).expect("should be able to read this test file via THIS_FILE")
}

/// Creates base/src/ holding placeholders for every file a COMPLETED generate
/// leaves behind — all EXPECTED_OUTPUTS and all GENERATED_HEADERS — and returns
/// the src_dir path. Deduplicates setup across stamp/output tests.
fn make_populated_src_dir(base: &Path) -> std::path::PathBuf {
    let src_dir = base.join(GENERATED_DIR);
    for name in EXPECTED_OUTPUTS.iter().chain(GENERATED_HEADERS) {
        let path = src_dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"placeholder").unwrap();
    }
    src_dir
}

#[test]
fn test_all_three_outputs_verified() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    assert_eq!(
        verify_outputs(&src_dir),
        Ok(()),
        "all files present should verify ok"
    );

    // Remove each file in turn: the error names exactly that file.
    for name in EXPECTED_OUTPUTS {
        let path = src_dir.join(name);
        std::fs::remove_file(&path).unwrap();
        let err = verify_outputs(&src_dir).unwrap_err();
        assert!(
            err.contains(name),
            "error should name the missing file '{}', got: {}",
            name,
            err
        );
        for present in EXPECTED_OUTPUTS.iter().filter(|p| *p != name) {
            assert!(
                !err.contains(present),
                "error must not name '{}', which is still present, got: {}",
                present,
                err
            );
        }
        std::fs::write(&path, b"placeholder").unwrap();
    }

    // Remove all of them: the error names every one.
    for name in EXPECTED_OUTPUTS {
        std::fs::remove_file(src_dir.join(name)).unwrap();
    }
    let err = verify_outputs(&src_dir).unwrap_err();
    for name in EXPECTED_OUTPUTS {
        assert!(
            err.contains(name),
            "error should name every missing file, '{}' absent from: {}",
            name,
            err
        );
    }
}

#[test]
fn test_verify_outputs_names_a_missing_generated_header() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    assert_eq!(
        verify_outputs(&src_dir),
        Ok(()),
        "control: a completed generate must verify"
    );

    for header in GENERATED_HEADERS {
        let path = src_dir.join(header);
        std::fs::remove_file(&path).unwrap();
        let err = verify_outputs(&src_dir)
            .expect_err("a generate that left a header missing must not verify");
        assert!(
            err.contains(header),
            "error should name the missing header '{header}', got: {err}"
        );
        for present in EXPECTED_OUTPUTS {
            assert!(
                !err.contains(present),
                "error must not name '{present}', which is still present, got: {err}"
            );
        }
        std::fs::write(&path, b"placeholder").unwrap();
    }
}

#[test]
fn test_out_dir_no_silent_fallback() {
    // Source-level regression guard: build.rs must NOT silently fall back to "." when
    // OUT_DIR is unset. Cargo always sets OUT_DIR for build scripts, so a missing value
    // means something is fundamentally wrong — we should panic, not pollute the source tree.
    let build_rs = std::fs::read_to_string(BUILD_RS)
        .expect("should be able to read build.rs from tree-sitter-reify crate root");

    // Find the line that reads the OUT_DIR env var (not comments mentioning OUT_DIR).
    let out_dir_line = build_rs
        .lines()
        .find(|line| {
            line.contains("env::var(\"OUT_DIR\")") || line.contains("env::var( \"OUT_DIR\")")
        })
        .expect("build.rs should contain a line reading env::var(\"OUT_DIR\")");

    assert!(
        !out_dir_line.contains("unwrap_or_else"),
        "OUT_DIR line must NOT use unwrap_or_else (silent fallback). \
         Cargo always sets OUT_DIR; a missing value should panic. Line: {}",
        out_dir_line
    );
    assert!(
        out_dir_line.contains("expect"),
        "OUT_DIR line must use .expect() for a clear panic message. Line: {}",
        out_dir_line
    );
}

#[test]
fn test_subprocess_timeout_kills_hung_process() {
    use std::time::Instant;

    let start = Instant::now();
    // Use 'sleep 30' to simulate a hung process, with 1s timeout.
    let result = run_with_timeout("sleep", &["30"], 1);
    let elapsed = start.elapsed();

    assert!(
        result.is_err(),
        "hung process should return error on timeout"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_lowercase().contains("timeout") || err.to_lowercase().contains("timed out"),
        "error should mention timeout, got: {}",
        err
    );
    // Should complete within ~2s (1s timeout + overhead), not 30s.
    assert!(
        elapsed.as_secs() < 5,
        "should have killed hung process quickly, but took {:?}",
        elapsed
    );
}

/// Whether this test binary's own descriptor `fd` is /dev/null, compared by
/// device and inode exactly as the shell's `-ef` does. On /dev/null a
/// discarded child descriptor and an inherited one are the same file, so the
/// stdio probes below cannot tell them apart.
#[cfg(target_os = "linux")]
fn own_fd_is_dev_null(fd: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (
        std::fs::metadata(format!("/proc/self/fd/{fd}")),
        std::fs::metadata("/dev/null"),
    ) {
        (Ok(own), Ok(null)) => own.dev() == null.dev() && own.ino() == null.ino(),
        _ => false,
    }
}

#[test]
#[cfg(target_os = "linux")]
fn test_run_with_timeout_discards_child_stdout() {
    assert!(
        !own_fd_is_dev_null(1),
        "precondition: this test binary's own stdout is /dev/null, so a \
         discarded and an inherited child stdout are indistinguishable; rerun \
         without redirecting stdout to /dev/null"
    );
    let result = run_with_timeout(
        "sh",
        &[
            "-c",
            r#"[ /dev/fd/1 -ef /dev/null ] && ! [ /dev/fd/1 -ef "/proc/$PPID/fd/1" ]"#,
        ],
        10,
    );
    assert_eq!(
        result,
        Ok(()),
        "child stdout must be discarded (/dev/null, not the build script's own \
         stdout): cargo parses build-script stdout for `cargo:` directives, and a \
         pipe nobody drains can deadlock"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_run_with_timeout_inherits_child_stderr() {
    assert!(
        !own_fd_is_dev_null(2),
        "precondition: this test binary's own stderr is /dev/null, so an \
         inherited and a discarded child stderr are indistinguishable; rerun \
         without redirecting stderr to /dev/null"
    );
    let result = run_with_timeout(
        "sh",
        &[
            "-c",
            r#"[ /dev/fd/2 -ef "/proc/$PPID/fd/2" ] && ! [ /dev/fd/2 -ef /dev/null ]"#,
        ],
        10,
    );
    assert_eq!(
        result,
        Ok(()),
        "child stderr must be the build script's own stderr, so `tree-sitter \
         generate` diagnostics stay visible"
    );
}

#[test]
fn test_run_with_timeout_reports_nonzero_exit_code() {
    let err = run_with_timeout("sh", &["-c", "exit 97"], 10).unwrap_err();
    assert!(
        err.contains("exit code 97"),
        "a failing child's error should carry its exit code 97, got: {}",
        err
    );
}

#[test]
fn test_run_with_timeout_reports_spawn_failure() {
    let missing = "reify-no-such-binary-8553";
    let err = run_with_timeout(missing, &[], 10).unwrap_err();
    assert!(
        err.contains(missing),
        "a spawn failure should name the binary '{}', got: {}",
        missing,
        err
    );
}

#[test]
fn test_stamp_path_is_profile_independent() {
    // The verdict depends only on the OUT_DIR it is asked about, so two profiles
    // holding the same generation agree on it, and both see a grammar change.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let debug_out = dir.path().join("target/debug/build/out");
    let release_out = dir.path().join("target/release/build/out");
    let (debug_generated, debug_stamp) = make_current_out_dir(&debug_out, &key);
    let (release_generated, release_stamp) = make_current_out_dir(&release_out, &key);

    assert!(
        !needs_generate(Some(&key), &debug_stamp, &debug_generated),
        "debug profile: must NOT regenerate when its key stamp matches"
    );
    assert!(
        !needs_generate(Some(&key), &release_stamp, &release_generated),
        "release profile: must NOT regenerate when its key stamp matches"
    );

    let changed = generation_key(&"b".repeat(64), FIXTURE_CLI_VERSION);
    assert!(
        needs_generate(Some(&changed), &debug_stamp, &debug_generated),
        "debug profile: must regenerate after a grammar change"
    );
    assert!(
        needs_generate(Some(&changed), &release_stamp, &release_generated),
        "release profile: must regenerate after a grammar change"
    );
}

#[test]
fn test_stamp_shared_across_simulated_profiles() {
    // Stamp presence is per-OUT_DIR: one profile's key stamp vouches for nothing
    // in another's, until that profile writes its own.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let out_dir_1 = dir.path().join("target/debug/build/out");
    let out_dir_2 = dir.path().join("target/release/build/out");
    let (generated_1, stamp_1) = make_current_out_dir(&out_dir_1, &key);
    let generated_2 = make_attested_generated_dir(&out_dir_2);
    let stamp_2 = out_dir_2.join(KEY_STAMP_NAME);

    assert!(
        !needs_generate(Some(&key), &stamp_1, &generated_1),
        "OUT_DIR_1: must NOT regenerate when its key stamp matches"
    );
    assert!(
        needs_generate(Some(&key), &stamp_2, &generated_2),
        "OUT_DIR_2: must regenerate while it has no key stamp"
    );

    write_generation_stamps(&generated_2, &stamp_2, &key);

    assert!(
        !needs_generate(Some(&key), &stamp_1, &generated_1),
        "OUT_DIR_1: still must NOT regenerate"
    );
    assert!(
        !needs_generate(Some(&key), &stamp_2, &generated_2),
        "OUT_DIR_2: must NOT regenerate once its own stamps are written"
    );
}

/// Returns true when running as root (UID 0). Used to skip tests that rely on
/// DAC permission enforcement, which root/CAP_DAC_OVERRIDE bypasses.
#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: libc::getuid() is a trivial POSIX syscall. libc provides a
    // well-audited, platform-tested binding — uid_t maps to u32 on Linux/macOS/BSD.
    unsafe { libc::getuid() == 0 }
}

/// RAII guard that unconditionally restores write permissions on drop.
/// Prevents temp-directory leaks when assertions panic between
/// set_readonly(true) and the manual permission restore.
struct ReadonlyGuard {
    path: std::path::PathBuf,
}

impl ReadonlyGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for ReadonlyGuard {
    fn drop(&mut self) {
        if let Ok(meta) = std::fs::metadata(&self.path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            if let Err(e) = std::fs::set_permissions(&self.path, perms) {
                eprintln!(
                    "warning: ReadonlyGuard failed to restore permissions on {}: {}",
                    self.path.display(),
                    e
                );
            }
        }
    }
}

/// Extracts the source text of a test function identified by `fn_sig`.
///
/// Searches `source` for `fn_sig` and returns the slice from that point up to
/// (but not including) the next `\n#[test]` annotation, or to the end of
/// `source` if no subsequent test function exists.
///
/// The sub-slice offset arithmetic adds 1 when converting from
/// `fn_section[1..].find(...)` back to an index in `fn_section`, avoiding the
/// off-by-one that would clip the character immediately before the next `#[test]`.
/// Similarly the fallback uses `fn_section.len()` (not `len() - 1`) so that
/// the last function's closing `}` is always included.
fn extract_test_fn_body<'a>(source: &'a str, fn_sig: &str) -> Option<&'a str> {
    let fn_start = source.find(fn_sig)?;
    let fn_section = &source[fn_start..];
    // `find` on `fn_section[1..]` returns an offset relative to the sub-slice.
    // Adding 1 converts it back to an index in `fn_section`.
    let fn_end = fn_section[1..]
        .find("\n#[test]")
        .map(|p| p + 1)
        .unwrap_or(fn_section.len());
    Some(&fn_section[..fn_end])
}

/// Scans `source` for test functions annotated with `#[cfg(unix)]` and returns
/// their signatures (e.g. `"fn test_foo()"`).
///
/// Uses a line-by-line state machine: once `#[cfg(unix)]` is seen, the flag
/// `saw_cfg_unix` is set. Intermediate attribute/comment lines keep the flag
/// alive. When a line starting with `fn test_` is reached with the flag set,
/// the signature up to and including `()` is collected. Non-test `fn` lines or
/// a blank line clears the flag, preventing false positives from isolated
/// `#[cfg(unix)]` helper functions.
fn find_cfg_unix_test_fns(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut saw_cfg_unix = false;
    let mut saw_test = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(unix)]") {
            saw_cfg_unix = true;
        } else if trimmed == "#[test]" {
            saw_test = true;
        } else if trimmed.starts_with("fn test_") && saw_cfg_unix && saw_test {
            // Extract "fn name()" — everything up to and including the first ')'
            if let Some(end) = trimmed.find(')') {
                result.push(trimmed[..=end].to_string());
            }
            saw_cfg_unix = false;
            saw_test = false;
        } else if trimmed.starts_with('#') {
            // Another attribute — keep flags alive (e.g. #[allow(...)])
        } else if trimmed.starts_with("//") || trimmed.is_empty() {
            // Comment or blank line — keep flags alive
        } else {
            // Any other line (fn without test, let, etc.) resets the flags
            saw_cfg_unix = false;
            saw_test = false;
        }
    }
    result
}

/// Scans `source` for `#[test]` functions whose body passes `THIS_FILE` as a
/// call argument — i.e., the body contains the substring `(THIS_FILE)` — and
/// returns their signatures (e.g. `"fn test_foo()"`).
///
/// Uses the same line-by-line state machine as `find_cfg_unix_test_fns`: once
/// `#[test]` is seen, the flag `saw_test` is set. Intermediate
/// attribute/comment lines keep the flag alive. When a line starting with
/// `fn test_` is reached with the flag set, `extract_test_fn_body` is used to
/// check whether the function body contains `(THIS_FILE)` as a call argument,
/// the bare relative-path literal `"tests/build_logic_tests.rs"` (with literal
/// double-quote characters), or a call to `read_self_source()`. Only functions
/// that pass this body check are collected.
///
/// The triple criterion turns `test_self_read_paths_use_manifest_dir`'s loop
/// into a genuine cross-property check: a test discovered via the bare-path
/// criterion will fail the loop's assertion, catching regressions where the
/// bare relative path is used instead of THIS_FILE or read_self_source().
fn find_self_reading_test_fns(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut saw_test = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "#[test]" {
            saw_test = true;
        } else if trimmed.starts_with("fn test_") && saw_test {
            // Extract "fn name()" — everything up to and including the first ')'
            if let Some(end) = trimmed.find(')') {
                let sig = trimmed[..=end].to_string();
                // Collect if the function body contains (THIS_FILE) as a call argument
                // OR the bare relative-path literal "tests/build_logic_tests.rs"
                // (with literal quotes). The second criterion is intentionally broader
                // so that a test using the bare path is discovered — and then fails
                // the THIS_FILE assertion in test_self_read_paths_use_manifest_dir.
                if let Some(body) = extract_test_fn_body(source, &sig)
                    && (body.contains("(THIS_FILE)")
                        || body.contains("\"tests/build_logic_tests.rs\"")
                        || body.contains("read_self_source()"))
                {
                    result.push(sig);
                }
            }
            saw_test = false;
        } else if trimmed.starts_with('#') {
            // Another attribute — keep flag alive (e.g. #[allow(...)])
        } else if trimmed.starts_with("//") || trimmed.is_empty() {
            // Comment or blank line — keep flag alive
        } else {
            // Any other line (fn without test, let, etc.) resets the flag
            saw_test = false;
        }
    }
    result
}

/// Strips line-level comments from a single source line and returns the code portion.
///
/// Algorithm (line-level only — multi-line `/* */` tracking is not supported):
/// 1. Iteratively find `/*` … `*/` pairs on this line and remove them.
///    Unclosed `/*` (no matching `*/` on the same line) is left unchanged.
/// 2. Split on `//` and take the first segment (strips trailing `//` comments).
/// 3. Return the result.
fn strip_line_comments(line: &str) -> String {
    // Step 1: remove all complete /* ... */ pairs on this line.
    let mut s = line.to_string();
    while let Some(open) = s.find("/*") {
        // Search for */ starting after the opening /*
        if let Some(rel_close) = s[open + 2..].find("*/") {
            let close_end = open + 2 + rel_close + 2; // byte index past "*/"
            s.replace_range(open..close_end, "");
        } else {
            break; // unclosed block comment on this line — leave unchanged
        }
    }
    // Step 2: strip trailing // comment.
    if let Some(idx) = s.find("//") {
        s.truncate(idx);
    }
    s
}

/// Generic scanner: returns all lines in `source` whose non-comment code portion
/// contains any of the given `needles`, as `(1-based line number, original line)` pairs.
///
/// Each line is first passed through [`strip_line_comments`] so that patterns inside
/// `//` or `/* */` comments are ignored.  Only the code portion is checked.
///
/// `needles` should include any surrounding delimiter characters that must be present
/// (e.g., a needle of `"\"build.rs\""` matches the literal text `"build.rs"` with
/// surrounding double-quote characters).
///
/// **Self-avoidance**: callers that define needle strings in this file use escaped
/// quotes (`\"…\"`) in Rust string literals, which in the raw source text appear as
/// the two-character sequence `\"` — that does NOT match the unescaped `"` delimiter
/// encoded in the needle value, so helper definitions do not trigger false positives.
fn find_bare_literal_violations<'src>(
    source: &'src str,
    needles: &[&str],
) -> Vec<(usize, &'src str)> {
    source
        .lines()
        .enumerate()
        .filter(|(_i, line)| {
            let code = strip_line_comments(line);
            needles.iter().any(|&needle| code.contains(needle))
        })
        .map(|(i, line)| (i + 1, line))
        .collect()
}

/// Scans `source` for lines that contain a bare `"build.rs"` or `"./build.rs"` string
/// literal (with literal double-quote characters) in non-comment code, and returns the
/// matching lines as `(1-based line number, original line)` pairs.
///
/// Each line is first passed through [`strip_line_comments`] so that the patterns inside
/// `//` or `/* */` comments are ignored.  Only the code portion is checked.
///
/// **Self-avoidance**: pattern strings defined in this file use escaped quotes
/// (`\"build.rs\"`) in Rust string literals, which in the raw source text appear as
/// the two-character sequence `\"` — that does NOT match the unescaped `"` used as the
/// search boundary, so this helper's own source does not trigger a false positive.
fn find_bare_build_rs_violations(source: &str) -> Vec<(usize, &str)> {
    find_bare_literal_violations(source, &["\"build.rs\"", "\"./build.rs\""])
}

/// Scans `source` for lines that contain a bare `"tests/build_logic_tests.rs"` or
/// `"./tests/build_logic_tests.rs"` string literal (with literal double-quote characters)
/// in non-comment code, and returns matching lines as `(1-based line number, original line)`
/// pairs.
///
/// Each line is first passed through [`strip_line_comments`] so that patterns inside
/// `//` or `/* */` comments are ignored.  Only the code portion is checked.
///
/// Delegates to [`find_bare_literal_violations`] with both path variants.
///
/// **Self-avoidance**: pattern strings defined in this file use escaped quotes
/// (`\"tests/build_logic_tests.rs\"`) in Rust string literals, which in the raw source
/// text appear as the two-character sequence `\"` — that does NOT match the unescaped
/// `"` used as the search boundary, so this helper's own source does not trigger a
/// false positive.
fn find_bare_self_path_violations(source: &str) -> Vec<(usize, &str)> {
    find_bare_literal_violations(
        source,
        &[
            "\"tests/build_logic_tests.rs\"",
            "\"./tests/build_logic_tests.rs\"",
        ],
    )
}

#[test]
fn test_unix_permission_tests_have_root_guard() {
    // Source-level regression guard: every #[cfg(unix)] test function that
    // relies on DAC permission enforcement must contain an is_root() skip guard.
    // Without it, tests produce misleading failures under root/CAP_DAC_OVERRIDE.
    //
    // The set of unix test functions is discovered dynamically by
    // find_cfg_unix_test_fns so that newly added #[cfg(unix)] tests are
    // automatically checked without updating a hardcoded list.
    let source = read_self_source();

    let unix_test_fns = find_cfg_unix_test_fns(&source);

    // Sanity-check: the scanner must find at least 2 unix tests (the ones we
    // know about). This catches a broken scanner that silently returns empty.
    assert!(
        unix_test_fns.len() >= 2,
        "find_cfg_unix_test_fns should discover at least 2 #[cfg(unix)] test functions, \
         but found {:?}",
        unix_test_fns
    );

    for fn_sig in &unix_test_fns {
        let fn_body = extract_test_fn_body(&source, fn_sig)
            .unwrap_or_else(|| panic!("source should contain {}", fn_sig));

        assert!(
            fn_body.contains("is_root()"),
            "{} must contain an is_root() skip guard to prevent misleading failures \
             when running as root. Function body:\n{}",
            fn_sig,
            fn_body
        );
    }
}

#[test]
fn test_readonly_guard_drop_logs_error() {
    // Source-level regression guard: ReadonlyGuard::drop must log errors from
    // set_permissions via eprintln! rather than silently discarding with `let _ =`.
    //
    // Design decision: the two assertions below enforce a semantic (not syntactic) invariant:
    //   (1) errors must not be silently discarded (no `let _ = set_permissions`)
    //   (2) errors must be logged via eprintln!
    // Any equivalent error-handling form is acceptable — `if let Err(e) = ...`,
    // `match { Err(e) => eprintln!(...), ... }`, `.map_err(|e| eprintln!(...))`, etc.
    // Do NOT add assertions that enforce a specific syntactic form; see
    // `test_drop_logs_error_check_is_form_agnostic` for the enforcement of this rule.
    let source = read_self_source();

    // Extract the Drop impl for ReadonlyGuard
    let drop_start = source
        .find("impl Drop for ReadonlyGuard")
        .expect("source should contain Drop impl for ReadonlyGuard");
    let drop_section = &source[drop_start..];
    // Find the closing brace of the impl block (next unindented '}')
    let drop_end = drop_section
        .find("\n}\n")
        .expect("Drop impl should have a closing brace");
    let drop_impl = &drop_section[..drop_end];

    assert!(
        !drop_impl.contains("let _ = std::fs::set_permissions"),
        "ReadonlyGuard::drop must NOT silently discard set_permissions errors with `let _ =`. \
         Use `if let Err(e) = ... {{ eprintln!(...) }}` or an equivalent `match` form \
         that logs the error instead. \
         Found in Drop impl:\n{}",
        drop_impl
    );
    assert!(
        drop_impl.contains("eprintln!"),
        "ReadonlyGuard::drop must log set_permissions errors via eprintln!. \
         Found in Drop impl:\n{}",
        drop_impl
    );
}

#[test]
#[cfg(unix)] // set_readonly(true) on a directory only prevents file creation on Unix (POSIX);
// on Windows the readonly attribute does NOT block creating files within the directory.
fn test_readonly_guard_restores_on_drop() {
    if is_root() {
        eprintln!("skipping: test requires non-root user (root bypasses DAC permissions)");
        return;
    }
    // Verify that ReadonlyGuard's Drop impl restores write permissions.
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("guarded");
    std::fs::create_dir_all(&subdir).unwrap();

    // Make the directory read-only
    let mut perms = std::fs::metadata(&subdir).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&subdir, perms).unwrap();

    // Guard takes ownership of the path and restores permissions on drop
    {
        let _guard = ReadonlyGuard::new(subdir.clone());
        // While guard is alive, directory is still read-only
        assert!(
            std::fs::File::create(subdir.join("probe_while_guarded.txt")).is_err(),
            "directory should still be read-only while guard is alive"
        );
    }
    // After guard is dropped, directory should be writable again
    std::fs::File::create(subdir.join("probe_after_drop.txt"))
        .expect("directory should be writable after ReadonlyGuard is dropped");
}

#[test]
fn test_is_root_uses_libc_not_raw_ffi() {
    // Source-level regression guard: is_root() must use libc::getuid() rather than
    // a raw `unsafe extern "C" { fn getuid() -> u32; }` declaration.
    // Raw FFI declarations are fragile (no type-checked header), whereas libc provides
    // a well-audited, platform-tested binding.
    let source = read_self_source();

    let fn_start = source
        .find("fn is_root()")
        .expect("source should contain is_root() function");
    let fn_section = &source[fn_start..];
    // Grab just up to the next function definition to avoid false positives
    let fn_end = fn_section[1..]
        .find("\nfn ")
        .map(|p| p + 1)
        .unwrap_or(fn_section.len());
    let fn_body = &fn_section[..fn_end];

    assert!(
        !fn_body.contains("unsafe extern \"C\""),
        "is_root() must NOT use a raw `unsafe extern \"C\"` FFI declaration. \
         Use `libc::getuid()` from the libc crate instead. Found body:\n{}",
        fn_body
    );
    assert!(
        fn_body.contains("libc::getuid()"),
        "is_root() must use `libc::getuid()` from the libc crate. Found body:\n{}",
        fn_body
    );
}

#[test]
fn test_extract_test_fn_body_no_off_by_one() {
    // Tests for the extract_test_fn_body() helper.
    // This helper extracts the source text of a test function bounded by fn_sig.
    // Key correctness property: the sub-slice offset arithmetic must add 1 when
    // converting from `fn_section[1..].find(...)` back to an index in `fn_section`.

    // Case (a): middle function — body must include all content up to (not including)
    // the "\n#[test]" that introduces the next function.
    let src_middle = concat!(
        "#[test]\n",
        "fn test_first() {\n",
        "    let x = 1;\n",
        "}\n",
        "#[test]\n",
        "fn test_second() {\n",
        "    let y = 2;\n",
        "}\n",
        "#[test]\n",
        "fn test_third() {\n",
        "    let z = 3;\n",
        "}"
    );
    let body = extract_test_fn_body(src_middle, "fn test_second()");
    let body = body.expect("extract_test_fn_body should find test_second");
    assert!(
        body.contains("let y = 2;"),
        "body should contain function content; got: {:?}",
        body
    );
    // The off-by-one would clip the closing `}` of test_second; verify it's present.
    assert!(
        body.contains('}'),
        "body should include closing brace of test_second; got: {:?}",
        body
    );
    // Body must NOT bleed into the next function.
    assert!(
        !body.contains("let z = 3;"),
        "body must not include content from test_third; got: {:?}",
        body
    );

    // Case (b): last function — off-by-one `unwrap_or(len - 1)` would clip the
    // final character. The corrected version uses `unwrap_or(len)`.
    let src_last = concat!(
        "#[test]\n",
        "fn test_alpha() {\n",
        "    is_root();\n",
        "}\n",
        "#[test]\n",
        "fn test_omega() {\n",
        "    let last = true;\n",
        "}" // NOTE: no trailing newline — the off-by-one clips this `}`
    );
    let body_last = extract_test_fn_body(src_last, "fn test_omega()");
    let body_last = body_last.expect("extract_test_fn_body should find test_omega");
    assert!(
        body_last.ends_with('}'),
        "body of last function must include the closing brace (off-by-one would clip it); \
         got: {:?}",
        body_last
    );

    // Case (c): function not found returns None.
    let none = extract_test_fn_body(src_last, "fn test_nonexistent()");
    assert!(none.is_none(), "should return None for missing function");
}

#[test]
fn test_find_cfg_unix_test_fns_discovers_dynamically() {
    // Tests for the find_cfg_unix_test_fns() helper.
    // The helper should find all test functions annotated with #[cfg(unix)],
    // regardless of ordering of attributes.

    let synthetic_source = concat!(
        // A plain test — no #[cfg(unix)] — should be ignored.
        "#[test]\n",
        "fn test_plain_no_unix() {\n",
        "    let _ = 1;\n",
        "}\n\n",
        // A #[cfg(unix)] test — should be collected.
        "#[cfg(unix)]\n",
        "#[test]\n",
        "fn test_unix_one() {\n",
        "    if is_root() { return; }\n",
        "}\n\n",
        // A non-test #[cfg(unix)] function — should be ignored.
        "#[cfg(unix)]\n",
        "fn helper_unix_not_a_test() {\n",
        "    let _ = 2;\n",
        "}\n\n",
        // A #[test] before #[cfg(unix)] — should also be collected.
        "#[test]\n",
        "#[cfg(unix)]\n",
        "fn test_unix_two() {\n",
        "    if is_root() { return; }\n",
        "}\n\n",
        // A #[cfg(unix)] followed by a non-test fn — should be ignored.
        "#[cfg(unix)]\n",
        "fn not_a_test_fn() {}\n",
    );

    let fns = find_cfg_unix_test_fns(synthetic_source);

    assert!(
        fns.contains(&"fn test_unix_one()".to_string()),
        "should discover test_unix_one; got: {:?}",
        fns
    );
    assert!(
        fns.contains(&"fn test_unix_two()".to_string()),
        "should discover test_unix_two; got: {:?}",
        fns
    );
    assert!(
        !fns.contains(&"fn test_plain_no_unix()".to_string()),
        "should NOT include test_plain_no_unix (no #[cfg(unix)]); got: {:?}",
        fns
    );
    assert!(
        !fns.iter().any(|s| s.contains("helper_unix_not_a_test")),
        "should NOT include helper_unix_not_a_test (not a #[test]); got: {:?}",
        fns
    );
    assert!(
        !fns.iter().any(|s| s.contains("not_a_test_fn")),
        "should NOT include not_a_test_fn (not a #[test]); got: {:?}",
        fns
    );
}

#[test]
fn test_find_self_reading_test_fns_discovers_dynamically() {
    // Tests for the find_self_reading_test_fns() helper.
    // The helper should find all #[test] functions whose body calls a function
    // with THIS_FILE as an argument (i.e., body contains the substring "(THIS_FILE)").

    let synthetic_source = concat!(
        // Case (a): a test fn whose body contains read_to_string(THIS_FILE) — should be collected.
        "#[test]\n",
        "fn test_reads_this_file() {\n",
        "    let source = std::fs::read_to_string(THIS_FILE).unwrap();\n",
        "}\n\n",
        // Case (a2): a test fn whose body contains File::open(THIS_FILE) — should be collected.
        "#[test]\n",
        "fn test_opens_this_file() {\n",
        "    let f = File::open(THIS_FILE).unwrap();\n",
        "}\n\n",
        // Case (b): a non-test fn with (THIS_FILE) in body — should be ignored.
        "fn helper_with_this_file() {\n",
        "    let _ = fs::read(THIS_FILE).unwrap();\n",
        "}\n\n",
        // Case (c): a test fn without (THIS_FILE) in body — should be ignored.
        "#[test]\n",
        "fn test_no_this_file() {\n",
        "    let _ = 1;\n",
        "}\n\n",
        // Case (d): a test fn that mentions THIS_FILE only in a comment (no call form) — should be ignored.
        // The comment below contains the bare identifier THIS_FILE but not the
        // parenthesized form, so the scanner must not collect this function.
        "#[test]\n",
        "fn test_comment_only_mention() {\n",
        "    // This test does not call THIS_FILE as an argument\n",
        "    let _ = 2;\n",
        "}\n\n",
        // Case (e): a #[test] followed by an intermediate #[allow(unused)] attribute,
        // then a fn using (THIS_FILE) — should be collected. This exercises the state
        // machine's `starts_with('#')` branch that keeps `saw_test` alive across
        // intermediate attributes (previously uncovered).
        "#[test]\n",
        "#[allow(unused)]\n",
        "fn test_with_attr_gap() {\n",
        "    let _source = std::fs::read_to_string(THIS_FILE).unwrap();\n",
        "}\n\n",
        // Case (f): a #[test] fn whose body uses the bare relative path
        // "tests/build_logic_tests.rs" (no THIS_FILE) — should be collected by the
        // broadened criterion. This RED test forces the step-3 impl that adds
        // `|| body.contains("\"tests/build_logic_tests.rs\"")` to the scanner.
        //
        // Two contexts to keep distinct:
        //   Evaluated string — concat! fuses these string literals at compile time;
        //     the escaped \"...\" sequences in the fragment below become real
        //     double-quote characters in the runtime value, so the 28-char
        //     contiguous substring "tests/build_logic_tests.rs" (with literal
        //     quotes) IS present in the evaluated string. That is precisely why
        //     case (f) IS collected by the scanner.
        //   Raw source file on disk — the .rs file contains the two-character
        //     escape sequence backslash-quote rather than a bare double-quote,
        //     so the contiguous 28-char substring does NOT appear. This prevents
        //     false-positives when test_self_read_paths_use_manifest_dir scans
        //     the real file and encounters this very comment block.
        "#[test]\n",
        "fn test_with_bare_path() {\n",
        "    let _s = std::fs::read_to_string(\"tests/build_logic_tests.rs\").unwrap();\n",
        "}\n\n",
        // Case (g): a #[test] fn whose body calls read_self_source() with no direct
        // (THIS_FILE) usage — should be collected by the broadened criterion.
        // This RED test forces the step-2 impl that adds
        // `|| body.contains("read_self_source()")` to the scanner.
        "#[test]\n",
        "fn test_uses_read_self_source() {\n",
        "    let _s = read_self_source();\n",
        "}\n",
    );

    let fns = find_self_reading_test_fns(synthetic_source);

    assert!(
        fns.contains(&"fn test_reads_this_file()".to_string()),
        "should discover test_reads_this_file; got: {:?}",
        fns
    );
    assert!(
        fns.contains(&"fn test_opens_this_file()".to_string()),
        "should discover test_opens_this_file; got: {:?}",
        fns
    );
    assert!(
        !fns.iter().any(|s| s.contains("helper_with_this_file")),
        "should NOT include helper_with_this_file (not a #[test]); got: {:?}",
        fns
    );
    assert!(
        !fns.contains(&"fn test_no_this_file()".to_string()),
        "should NOT include test_no_this_file (no (THIS_FILE) in body); got: {:?}",
        fns
    );
    assert!(
        !fns.contains(&"fn test_comment_only_mention()".to_string()),
        "should NOT include test_comment_only_mention (THIS_FILE only in comment); got: {:?}",
        fns
    );
    assert!(
        fns.contains(&"fn test_with_attr_gap()".to_string()),
        "should discover test_with_attr_gap (intermediate #[allow] attribute between #[test] and fn — \
         the state machine's starts_with('#') branch keeps saw_test alive); got: {:?}",
        fns
    );
    assert!(
        fns.contains(&"fn test_with_bare_path()".to_string()),
        "should discover test_with_bare_path (broadened criterion: body contains the bare relative-path \
         literal \"tests/build_logic_tests.rs\" with literal quotes); got: {:?}",
        fns
    );
    assert!(
        fns.contains(&"fn test_uses_read_self_source()".to_string()),
        "should discover test_uses_read_self_source (broadened criterion: body contains \
         read_self_source() call); got: {:?}",
        fns
    );
    assert_eq!(
        fns.len(),
        5,
        "expected exactly 5 discovered functions (a=test_reads_this_file, a2=test_opens_this_file, \
         e=test_with_attr_gap, f=test_with_bare_path, g=test_uses_read_self_source); got: {:?}",
        fns
    );
}

#[test]
fn test_self_read_paths_use_manifest_dir() {
    // Meta-test / regression guard: each source-self-inspection test that reads
    // this file must use the `THIS_FILE` constant rather than the bare relative path
    // `"tests/build_logic_tests.rs"`.
    // (The bare path above is backtick-quoted so that a future blanket-guard
    // assertion scanning for the literal path string does not false-match
    // this describing comment.)
    // A bare relative path is fragile — it depends on the working directory from which
    // `cargo test` is invoked, causing failures when tests are run from outside the
    // crate root (e.g., workspace-level `cargo test -p tree-sitter-reify`).
    //
    // THIS_FILE resolves to an absolute path via CARGO_MANIFEST_DIR at compile time,
    // so it is safe regardless of invocation directory.
    //
    // The set of self-reading test functions is discovered dynamically by
    // find_self_reading_test_fns so that newly added self-reading tests are
    // automatically checked without updating a hardcoded list.
    // This test excludes itself from the discovered set to avoid circularity
    // (its body contains (THIS_FILE) in both the read call and assertion code).
    //
    // The loop below is a genuine cross-property check: find_self_reading_test_fns
    // uses a broadened criterion (THIS_FILE call-form OR bare relative-path literal),
    // while the loop asserts only the THIS_FILE criterion. A test discovered via the
    // bare-path criterion would pass the scanner but fail this assertion, catching
    // the exact regression the guard is designed to prevent.
    let source = read_self_source();

    let self_reading_fns = find_self_reading_test_fns(&source);

    // Exclude this meta-test itself to avoid circularity.
    let self_reading_fns: Vec<String> = self_reading_fns
        .into_iter()
        .filter(|sig| !sig.contains("test_self_read_paths_use_manifest_dir"))
        .collect();

    // Sanity-check: the scanner must find at least 6 self-reading tests. This catches a
    // broken scanner that silently returns empty or near-empty. The threshold is a
    // lower bound rather than an exact count because the real file may gain more
    // self-reading tests over time.
    assert!(
        self_reading_fns.len() >= 6,
        "find_self_reading_test_fns should discover at least 6 self-reading test functions \
         after excluding this meta-test; but found {:?}",
        self_reading_fns
    );

    for fn_sig in &self_reading_fns {
        let fn_body = extract_test_fn_body(&source, fn_sig)
            .unwrap_or_else(|| panic!("source should contain {}", fn_sig));

        assert!(
            fn_body.contains("(THIS_FILE)") || fn_body.contains("read_self_source()"),
            "{} must read the test file via THIS_FILE or read_self_source() rather than \
             a bare relative path. Function body:\n{}",
            fn_sig,
            fn_body
        );
    }
}

#[test]
fn test_self_path_constants_use_manifest_dir() {
    // Regression guard: the `THIS_FILE` and `BUILD_RS` constants must be defined using
    // the `env!` macro on `CARGO_MANIFEST_DIR` for portable compile-time path resolution.
    //
    // Single responsibility: asserts only that the constant *definitions* use the portable
    // env! macro form — orthogonal to `test_self_read_paths_use_manifest_dir`, which asserts
    // that each self-reading *test function* references `THIS_FILE` (not a bare relative path).
    // Splitting these checks yields unambiguous failure messages: a broken constant definition
    // fails only here; a test that bypasses THIS_FILE fails only in the other test.
    //
    // The count-based check (>= 2) requires the exact macro invocation to appear at least twice,
    // once for each constant, so neither definition can silently regress to a hardcoded path.
    let source = read_self_source();

    assert!(
        source.matches("env!(\"CARGO_MANIFEST_DIR\")").count() >= 2,
        "THIS_FILE and BUILD_RS constants must be defined using env!(\"CARGO_MANIFEST_DIR\") \
         for portable compile-time path resolution; at least two occurrences required \
         (one per constant)"
    );
}

#[test]
fn test_self_path_constants_guard_is_not_vacuous() {
    // Meta-test: verifies that the count-based assertion in
    // `test_self_path_constants_use_manifest_dir` is non-vacuous AND orthogonal.
    // Design: the main test uses a lower-bound check (guards against regression);
    // this meta-test uses an exact-count check `count == 2` (guards against inflation).
    // Together they are orthogonal: a drop below 2 fails only the main test; an unexpected
    // addition above 2 fails only this meta-test, so any single failure has a unique cause.
    let source = read_self_source();
    let count = source.matches("env!(\"CARGO_MANIFEST_DIR\")").count();
    assert!(
        count == 2,
        "expected exactly 2 occurrences of env!(\"CARGO_MANIFEST_DIR\") in this file \
         (one for THIS_FILE const, one for BUILD_RS const), but found {}",
        count
    );
}

#[test]
fn test_path_constants_guard_uses_exact_count() {
    // Meta-meta-test: `test_self_path_constants_guard_is_not_vacuous` must use
    // `count == 2` (exact match) rather than `count >= 2` (lower bound).
    // Using an exact count makes the meta-test orthogonal to the main test:
    // the main test (`test_self_path_constants_use_manifest_dir`) guards against
    // regression (count dropping below 2); the meta-test guards against inflation
    // (unexpected additions that could raise the count and mask a regression).
    let source = std::fs::read_to_string(THIS_FILE)
        .expect("should be able to read this test file via THIS_FILE");

    let body = extract_test_fn_body(
        &source,
        "fn test_self_path_constants_guard_is_not_vacuous()",
    )
    .expect("source should contain test_self_path_constants_guard_is_not_vacuous");

    assert!(
        body.contains("count == 2"),
        "test_self_path_constants_guard_is_not_vacuous must assert `count == 2` \
         (exact match) so that the meta-test is orthogonal to the main test's `>= 2` \
         lower-bound check. Function body:\n{}",
        body
    );

    assert!(
        !body.contains("count >= 2"),
        "test_self_path_constants_guard_is_not_vacuous must NOT use `count >= 2` \
         (lower bound), as that would be redundant with the main test's assertion. \
         Use `count == 2` instead. Function body:\n{}",
        body
    );
}

#[test]
fn test_self_reading_count_comment_accuracy() {
    // Meta-test: if `test_self_read_paths_use_manifest_dir` contains a parenthetical
    // of the form `(actual count is N)`, the claimed N must match the real count
    // returned by `find_self_reading_test_fns` (with the same self-exclusion filter).
    // If no such parenthetical is present, this test passes vacuously — removing the
    // parenthetical is the preferred fix for a stale claim.
    let source = std::fs::read_to_string(THIS_FILE)
        .expect("should be able to read this test file via THIS_FILE");

    let body = extract_test_fn_body(&source, "fn test_self_read_paths_use_manifest_dir()")
        .expect("source should contain test_self_read_paths_use_manifest_dir");

    // Compute the real count with the same self-exclusion filter used in the test.
    let real_count = find_self_reading_test_fns(&source)
        .into_iter()
        .filter(|sig| !sig.contains("test_self_read_paths_use_manifest_dir"))
        .count();

    // If the body contains a `(actual count is N)` parenthetical, it must be accurate.
    let prefix = "(actual count is ";
    if let Some(start) = body.find(prefix) {
        let after = &body[start + prefix.len()..];
        let end = after
            .find(')')
            .expect("parenthetical must be closed with ')'");
        let claimed: usize = after[..end]
            .trim()
            .parse()
            .expect("value after '(actual count is ' must be a number");
        assert_eq!(
            claimed, real_count,
            "test_self_read_paths_use_manifest_dir claims '(actual count is {})' but \
             find_self_reading_test_fns (after self-exclusion) returns {}. \
             Either update the claim or remove the parenthetical entirely.",
            claimed, real_count
        );
    }
    // No parenthetical present — passes vacuously.
}

#[test]
fn test_drop_logs_error_check_is_form_agnostic() {
    // Meta-test / regression guard: `test_readonly_guard_drop_logs_error` must enforce
    // *semantic* invariants only (no silent discard + must log), not *syntactic* ones.
    // A future contributor must not add assertions like `contains("if let Err"` or
    // `contains("match "` that enforce a specific error-handling form — any equivalent
    // form that logs the error is acceptable.
    let source = read_self_source();

    let drop_logs_body = extract_test_fn_body(&source, "fn test_readonly_guard_drop_logs_error()")
        .expect("source should contain test_readonly_guard_drop_logs_error");

    assert!(
        !drop_logs_body.contains("contains(\"if let Err\""),
        "test_readonly_guard_drop_logs_error must NOT assert a specific syntactic form \
         via contains(\"if let Err\"). The assertion pair must remain semantic (not syntactic): \
         (1) errors must not be silently discarded, (2) errors must be logged via eprintln!. \
         Any equivalent error-handling form is acceptable. Function body:\n{}",
        drop_logs_body
    );

    assert!(
        !drop_logs_body.contains("contains(\"match \""),
        "test_readonly_guard_drop_logs_error must NOT assert a specific syntactic form \
         via contains(\"match \"). The assertion pair must remain semantic (not syntactic): \
         (1) errors must not be silently discarded, (2) errors must be logged via eprintln!. \
         Any equivalent error-handling form is acceptable. Function body:\n{}",
        drop_logs_body
    );

    assert!(
        drop_logs_body.contains("semantic (not syntactic)"),
        "test_readonly_guard_drop_logs_error must contain a design-decision comment \
         with the phrase 'semantic (not syntactic)' explaining why only semantic invariants \
         are enforced (no silent discard + must log), not syntactic form. \
         Add a comment to the function body using that exact phrase. \
         Function body:\n{}",
        drop_logs_body
    );
}

#[test]
fn test_strip_line_comments() {
    // Full-line `// comment` → empty string (split on `//` yields "")
    assert_eq!(strip_line_comments("// this is a comment"), "");

    // Code with trailing `// comment` → code portion only (with trailing space)
    assert_eq!(
        strip_line_comments("    let x = 1; // comment"),
        "    let x = 1; "
    );

    // Code with closed `/* inline comment */` → comment block removed
    assert_eq!(strip_line_comments("a /* x */ b"), "a  b");

    // Unclosed `/*` with no matching `*/` on same line → left unchanged (line-level only)
    assert_eq!(strip_line_comments("/* unclosed"), "/* unclosed");

    // Line with no comments → returned unchanged
    assert_eq!(strip_line_comments("let x = 1;"), "let x = 1;");

    // Code before and after `/* comment */` are both preserved
    assert_eq!(strip_line_comments("foo(/* polling */ bar)"), "foo( bar)");

    // Known limitation: // inside string literals is treated as comment start
    assert_eq!(
        strip_line_comments(r#"let s = "//"; let x = 1;"#),
        r#"let s = ""#
    );

    // Multiple /* */ blocks on one line — loop handles successive pairs
    assert_eq!(strip_line_comments("a /* x */ b /* y */ c"), "a  b  c");

    // Block comment then line comment — exercises both steps in sequence
    assert_eq!(
        strip_line_comments("code /* block */ more // line"),
        "code  more "
    );
}

#[test]
fn test_find_bare_literal_violations_generic() {
    // Helper: returns the detected line numbers for a given source and needles.
    fn line_nums(source: &str, needles: &[&str]) -> Vec<usize> {
        find_bare_literal_violations(source, needles)
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    }

    // ── Single needle: detection in non-comment code ─────────────────────────
    // Use escaped quotes so the raw source contains \" (backslash+quote) rather
    // than a bare unescaped quote, preventing a future guard test from flagging
    // this code as a violation.
    assert!(
        !line_nums("read_to_string(\"needle.rs\")", &["\"needle.rs\""]).is_empty(),
        "needle in code should be detected"
    );

    // ── Single needle: non-detection in // comment ────────────────────────────
    assert!(
        line_nums("// read_to_string(\"needle.rs\")", &["\"needle.rs\""]).is_empty(),
        "needle inside // comment must not be detected"
    );

    // ── Single needle: non-detection in /* */ block comment ───────────────────
    assert!(
        line_nums("/* read_to_string(\"needle.rs\") */", &["\"needle.rs\""]).is_empty(),
        "needle inside /* */ block comment must not be detected"
    );

    // ── 1-based line numbering on multi-line input ────────────────────────────
    let src = "ok\nread_to_string(\"needle.rs\")\nok";
    let result = find_bare_literal_violations(src, &["\"needle.rs\""]);
    assert_eq!(result.len(), 1, "should detect exactly one violation");
    assert_eq!(
        result[0].0, 2,
        "line numbering should be 1-based (hit is on line 2)"
    );

    // ── Multiple needles: any match triggers detection ────────────────────────
    let multi = find_bare_literal_violations(
        "File::open(\"other.rs\")",
        &["\"needle.rs\"", "\"other.rs\""],
    );
    assert!(
        !multi.is_empty(),
        "second needle should also trigger detection"
    );

    // ── No false positives when no needle matches ─────────────────────────────
    assert!(
        line_nums("let x = 5;", &["\"needle.rs\""]).is_empty(),
        "source with no needle must not produce violations"
    );
}

/// Shared test helper for bare-literal violation detection functions.
///
/// For each `(source_line, description)` pair in `positive_cases`: asserts that
/// `finder_fn` returns at least one violation, using `description` in the failure
/// message.
/// For each `(source_line, description)` pair in `negative_cases`: asserts that
/// `finder_fn` returns no violations.
fn assert_bare_literal_detection(
    finder_fn: fn(&str) -> Vec<(usize, &str)>,
    positive_cases: &[(&str, &str)],
    negative_cases: &[(&str, &str)],
) {
    for (input, label) in positive_cases {
        assert!(
            !finder_fn(input).is_empty(),
            "{label}: expected a violation in {input:?}"
        );
    }
    for (input, label) in negative_cases {
        assert!(
            finder_fn(input).is_empty(),
            "{label}: expected no violations in {input:?}"
        );
    }
}

#[test]
fn test_find_bare_self_path_violations() {
    // Use escaped quotes so the raw source contains \" rather than a bare ".
    // At runtime the values contain bare quotes, so the helper detects them.
    assert_bare_literal_detection(
        find_bare_self_path_violations,
        &[
            (
                "read_to_string(\"tests/build_logic_tests.rs\")",
                "bare path without ./ should be detected",
            ),
            (
                "read_to_string(\"./tests/build_logic_tests.rs\")",
                "bare ./relative path should be detected",
            ),
            (
                "File::open(\"tests/build_logic_tests.rs\")",
                "File::open variant should be detected",
            ),
            (
                "read_to_string( \"tests/build_logic_tests.rs\" )",
                "whitespace variant should be detected",
            ),
        ],
        &[
            (
                "read_to_string(THIS_FILE)",
                "constant usage must not be detected",
            ),
            (
                "// read_to_string(\"tests/build_logic_tests.rs\")",
                "full-line // comment must not be detected",
            ),
            (
                "let x = 0; // read_to_string(\"tests/build_logic_tests.rs\")",
                "inline // comment must not be detected",
            ),
            (
                "/* read_to_string(\"tests/build_logic_tests.rs\") */",
                "/* */ block comment must not be detected",
            ),
        ],
    );

    // self-avoidance: escaped quotes in a string literal definition do not
    // form the bare-quote pattern the helper searches for.
    let self_ref = r#"let p = "\"tests/build_logic_tests.rs\"";"#;
    assert!(
        find_bare_self_path_violations(self_ref).is_empty(),
        "escaped-quote self-reference must not be detected"
    );

    // Multi-line 1-based line numbering check.
    let src = "ok\nread_to_string(\"tests/build_logic_tests.rs\")\nok";
    let hits = find_bare_self_path_violations(src);
    assert_eq!(hits.len(), 1, "should detect exactly one violation");
    assert_eq!(
        hits[0].0, 2,
        "line numbering must be 1-based (hit is on line 2)"
    );
}

#[test]
fn test_find_bare_build_rs_violations() {
    // Use escaped quotes so the raw source contains \" rather than a bare ".
    // At runtime the values contain bare quotes, so the helper detects them.
    assert_bare_literal_detection(
        find_bare_build_rs_violations,
        &[
            (
                "read_to_string(\"build.rs\")",
                "exact call should be detected",
            ),
            (
                "read_to_string( \"build.rs\" )",
                "whitespace variant should be detected",
            ),
            (
                "read_to_string(\"./build.rs\")",
                "relative path variant should be detected",
            ),
            (
                "File::open(\"build.rs\")",
                "File::open variant should be detected",
            ),
        ],
        &[
            (
                "read_to_string(BUILD_RS)",
                "constant usage must not be detected",
            ),
            (
                "// read_to_string(\"build.rs\")",
                "full-line // comment must not be detected",
            ),
            (
                "let x = 0; // read_to_string(\"build.rs\")",
                "inline // comment must not be detected",
            ),
            (
                "/* read_to_string(\"build.rs\") */",
                "/* */ block comment must not be detected",
            ),
        ],
    );

    // self-avoidance: escaped quotes in string literal definition.
    let self_reference = r#"let p = "\"build.rs\"";"#;
    assert!(
        find_bare_build_rs_violations(self_reference).is_empty(),
        "escaped-quote self-reference must not be detected"
    );

    // Multi-line input: verify 1-based line numbering.
    let hits = find_bare_build_rs_violations("ok\nread_to_string(\"build.rs\")\nok");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, 2);
}

#[test]
#[allow(clippy::type_complexity)]
fn test_no_bare_relative_path_reads() {
    // Source-level regression guard: this file must not contain any bare
    // relative-path string literals for the files it reads (build.rs or this
    // test file itself) in non-comment code. Each reader must use the
    // compile-time constants BUILD_RS or THIS_FILE (resolved via
    // CARGO_MANIFEST_DIR) rather than fragile relative paths.
    //
    // Detection strips `//` and `/* */` comments before scanning each line
    // so that inline-commented examples do not trigger false positives.
    //
    // Note: path-pattern strings in this file use escaped quotes in Rust string
    // literals, so the raw source text contains \" rather than an unescaped ",
    // and the helpers find no self-match.
    let source = read_self_source();

    // Each entry: (violation scanner, path description, constant to use instead).
    let guards: &[(fn(&str) -> Vec<(usize, &str)>, &str, &str)] = &[
        (
            find_bare_build_rs_violations,
            "\"build.rs\" or \"./build.rs\"",
            "BUILD_RS",
        ),
        (
            find_bare_self_path_violations,
            "\"tests/build_logic_tests.rs\" or \"./tests/build_logic_tests.rs\"",
            "THIS_FILE",
        ),
    ];

    for (finder_fn, path_desc, constant_name) in guards {
        let violations = finder_fn(&source);
        assert!(
            violations.is_empty(),
            "Found {} bare {} literal(s) in non-comment code. \
             Use the {} constant instead (defined via CARGO_MANIFEST_DIR). \
             Violations:\n{}",
            violations.len(),
            path_desc,
            constant_name,
            violations
                .iter()
                .map(|(n, l)| format!("  line {}: {}", n, l.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn test_no_bare_relative_self_path_reads() {
    // Source-level regression guard: this file must not contain any bare
    // "tests/build_logic_tests.rs" string literals in non-comment code.  Every
    // source-inspection test that reads this file must use the `THIS_FILE`
    // constant (resolved at compile time via CARGO_MANIFEST_DIR) rather than a
    // fragile relative path.
    //
    // Detection uses find_bare_self_path_violations, which strips `//` and
    // `/* */` comments before scanning each line — so describing comments that
    // mention the path (e.g. using escaped form \"tests/build_logic_tests.rs\")
    // are ignored.
    //
    // Note: pattern strings in this file use escaped quotes in Rust string
    // literals, so the raw source text contains \" rather than an unescaped ",
    // and the helper finds no self-match.
    let source = read_self_source();

    let violations = find_bare_self_path_violations(&source);

    assert!(
        violations.is_empty(),
        "Found {} bare \"tests/build_logic_tests.rs\" literal(s) in non-comment code. \
         Use the THIS_FILE constant instead (defined via CARGO_MANIFEST_DIR). \
         Violations:\n{}",
        violations.len(),
        violations
            .iter()
            .map(|(n, l)| format!("  line {}: {}", n, l.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── Generated-output manifest primitives (`#6992`) ───────────────────────────
//
// The three stamps in this pipeline used to attest only `grammar.js`:
// `src/.grammar_hash.stamp` and `$OUT_DIR/grammar_hash.stamp` both said "the
// grammar hashes to X" and then let a merely-EXISTING `src/parser.c` ride along
// on that claim. A merge that restores an older grammar.js beside a parser.c
// generated from a newer one therefore produces a stamp that actively vouches
// for the wrong parser, and the mtime heuristic that was supposed to catch it
// is erased by warm-lane seeding (every file in this lane is stamped
// `Jan 1 2020`, so `output_mtime > stamp_mtime` is universally false).
//
// The cure is a manifest that names the generated outputs by CONTENT. These
// tests drive the shared primitives from `build_support.rs` — the same code
// `build.rs` runs — never a replica.

/// True when this host can hash at all.
///
/// Mirrors `sha256_of`'s three-outcome contract: `Ok(None)` is the host-wide
/// "no sha256sum and no shasum" fact, and every manifest assertion below is
/// vacuous there. Skipping is the honest response — a panic would report an
/// environment gap as a defect.
fn hasher_available() -> bool {
    matches!(sha256_of(BUILD_RS), Ok(Some(_)))
}

/// Render a manifest naming `rels` inside `src_dir`, hashing each from disk.
///
/// Deliberately hashes the REAL bytes rather than accepting a caller-supplied
/// digest: a fixture that can mint an arbitrary hash proves nothing about the
/// predicate under test.
fn render_manifest_for(src_dir: &Path, rels: &[&str]) -> String {
    let mut entries: Vec<(String, String)> = Vec::new();
    for rel in rels {
        let hash = sha256_of_path(&src_dir.join(rel))
            .expect("fixture file must hash")
            .expect("hasher_available() was checked first");
        entries.push((hash, (*rel).to_string()));
    }
    outputs_manifest_render(&entries)
}

/// `src_dir/.generated_outputs.stamp` path — the sibling stamp under test.
fn outputs_stamp_of(src_dir: &Path) -> std::path::PathBuf {
    src_dir.join(OUTPUTS_STAMP_NAME)
}

#[test]
fn test_outputs_manifest_render_is_sorted_and_byte_shaped() {
    // The format is REUSED from write_inputs_stamp (build_support.rs) verbatim:
    // "<hash>  <relpath>\n" lines, sorted by relpath, two spaces. Both the
    // Rust and the shell halves parse it, so the bytes are the contract.
    let rendered = outputs_manifest_render(&[
        ("cccc".to_string(), "parser.c".to_string()),
        ("aaaa".to_string(), "grammar.json".to_string()),
        ("bbbb".to_string(), "node-types.json".to_string()),
    ]);
    assert_eq!(
        rendered, "aaaa  grammar.json\nbbbb  node-types.json\ncccc  parser.c\n",
        "manifest must be sorted by relpath and use the two-space separator"
    );
}

#[test]
fn test_outputs_manifest_parse_round_trips_render() {
    let entries = vec![
        ("aaaa".to_string(), "grammar.json".to_string()),
        ("bbbb".to_string(), "node-types.json".to_string()),
        ("cccc".to_string(), "parser.c".to_string()),
    ];
    let parsed = outputs_manifest_parse(&outputs_manifest_render(&entries))
        .expect("a rendered manifest must parse");
    assert_eq!(parsed, entries, "render -> parse must be lossless");
}

#[test]
fn test_outputs_manifest_matches_true_for_untouched_outputs() {
    // (a) The legitimate fast path: a manifest minted from the outputs on disk
    // must verify against those same outputs. Without this, the fix would be a
    // permanent regeneration loop rather than a staleness detector.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    std::fs::write(
        outputs_stamp_of(&src_dir),
        render_manifest_for(&src_dir, EXPECTED_OUTPUTS),
    )
    .unwrap();

    assert!(
        outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
        "a manifest minted from these exact bytes must verify"
    );
}

#[test]
fn test_outputs_manifest_matches_false_after_one_byte_mutation() {
    // (b) THE DEFECT, reduced to one byte: parser.c no longer matches what the
    // manifest recorded. Content is the only signal that survives warm-lane
    // mtime normalization, so this is the assertion the whole task rests on.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    std::fs::write(
        outputs_stamp_of(&src_dir),
        render_manifest_for(&src_dir, EXPECTED_OUTPUTS),
    )
    .unwrap();

    let parser = src_dir.join("parser.c");
    let mut bytes = std::fs::read(&parser).unwrap();
    bytes[0] ^= 0x01;
    std::fs::write(&parser, &bytes).unwrap();

    assert!(
        !outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
        "a single mutated byte in parser.c must invalidate the manifest"
    );
}

#[test]
fn test_outputs_manifest_matches_false_when_manifest_missing() {
    // (c) No manifest is UNPROVEN, and unproven must read as stale. The old
    // code treated the absence of evidence as evidence of freshness.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    assert!(
        !outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
        "an absent manifest must never vouch for the outputs beside it"
    );
}

#[test]
fn test_outputs_manifest_matches_false_when_an_expected_output_is_unlisted() {
    // (d) A manifest covering 2 of the 3 outputs attests nothing about the
    // third — so it must not verify. A per-entry loop with no set check would
    // pass here, which is exactly the hole being closed.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    std::fs::write(
        outputs_stamp_of(&src_dir),
        render_manifest_for(&src_dir, &["grammar.json", "node-types.json"]),
    )
    .unwrap();

    assert!(
        !outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
        "a manifest that omits parser.c must not verify the output set"
    );
}

#[test]
fn test_outputs_manifest_matches_false_on_unexpected_relpath() {
    // (e) The other half of the set check: an entry outside EXPECTED_OUTPUTS
    // means the manifest describes a different output set than the one this
    // build script produces, so it cannot be trusted for this one.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    std::fs::write(src_dir.join("stowaway.c"), b"placeholder").unwrap();

    let mut rels: Vec<&str> = EXPECTED_OUTPUTS.to_vec();
    rels.push("stowaway.c");
    std::fs::write(
        outputs_stamp_of(&src_dir),
        render_manifest_for(&src_dir, &rels),
    )
    .unwrap();

    assert!(
        !outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
        "a manifest naming a file outside EXPECTED_OUTPUTS must not verify"
    );
}

#[test]
fn test_outputs_manifest_matches_false_on_malformed_line_without_panicking() {
    // (f) A truncated write (killed generate, full disk) must produce a plain
    // FALSE, never a panic: this predicate runs inside a build script, where a
    // panic is a hard build failure rather than a recoverable regeneration.
    if !hasher_available() {
        return;
    }
    for corrupt in [
        "not-a-manifest-line\n",
        "deadbeef\n",
        "deadbeef parser.c\n",
        "  parser.c\n",
        "deadbeef  \n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = make_populated_src_dir(dir.path());
        std::fs::write(outputs_stamp_of(&src_dir), corrupt).unwrap();
        assert!(
            !outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
            "malformed manifest {:?} must read as stale, not as current",
            corrupt
        );
        assert!(
            outputs_manifest_parse(corrupt).is_none(),
            "malformed manifest {:?} must fail to parse rather than panic",
            corrupt
        );
    }
}

#[test]
fn test_outputs_manifest_matches_false_on_empty_manifest() {
    // (g) An empty file is the classic crash-mid-write residue: created by the
    // open, never filled. It names no outputs, so it proves nothing.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src_dir = make_populated_src_dir(dir.path());
    std::fs::write(outputs_stamp_of(&src_dir), "").unwrap();

    assert!(
        !outputs_manifest_matches(&outputs_stamp_of(&src_dir), &src_dir),
        "an empty manifest must not verify the outputs beside it"
    );
}

// ── Generation into OUT_DIR (`#9896`) ────────────────────────────────────────
//
// build.rs generates into `$OUT_DIR/src` and compiles from there, so every
// generated source lives beside the archive it becomes. `needs_generate` is the
// one predicate deciding whether that directory holds what this build script
// would generate: its outputs, its headers, a parser at the pinned ABI, a
// manifest matching those bytes, and a key stamp naming the grammar and CLI.

/// A CLI version line for fixture keys.
const FIXTURE_CLI_VERSION: &str = "tree-sitter 0.26.8 (fixture)";

/// A key for a grammar whose sha256 is all `a`s, generated by the fixture CLI.
fn fixture_key() -> String {
    generation_key(&"a".repeat(64), FIXTURE_CLI_VERSION)
}

/// A `parser.c` body declaring `abi` the way `tree-sitter generate` does.
fn parser_c_declaring(abi: &str) -> String {
    format!(
        "#include \"tree_sitter/parser.h\"\n\n#define LANGUAGE_VERSION {abi}\n#define STATE_COUNT 1\n"
    )
}

/// An ABI the runtime is not pinned to: the one after it.
fn unpinned_abi() -> String {
    let pinned: u32 = GENERATED_PARSER_ABI.parse().unwrap();
    (pinned + 1).to_string()
}

/// `make_populated_src_dir` with a parser.c at the pinned ABI, and no stamps.
fn make_pinned_generated_dir(out_dir: &Path) -> std::path::PathBuf {
    let generated = make_populated_src_dir(out_dir);
    std::fs::write(
        generated.join("parser.c"),
        parser_c_declaring(GENERATED_PARSER_ABI),
    )
    .unwrap();
    generated
}

/// `make_pinned_generated_dir` plus the `.generated_outputs.stamp` attesting its
/// bytes: what a completed generate leaves, minus the key stamp.
///
/// Callers must gate on `hasher_available()` first, since the manifest needs a
/// hasher.
fn make_attested_generated_dir(out_dir: &Path) -> std::path::PathBuf {
    let generated = make_pinned_generated_dir(out_dir);
    std::fs::write(
        outputs_stamp_of(&generated),
        render_manifest_for(&generated, EXPECTED_OUTPUTS),
    )
    .unwrap();
    generated
}

/// The fully consistent OUT_DIR: an attested generated dir, and a key stamp
/// holding exactly `key`. Returns `(generated_dir, key_stamp)`.
fn make_current_out_dir(out_dir: &Path, key: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let generated = make_attested_generated_dir(out_dir);
    let key_stamp = out_dir.join(KEY_STAMP_NAME);
    std::fs::write(&key_stamp, key).unwrap();
    (generated, key_stamp)
}

#[test]
fn test_needs_generate_false_in_the_fully_consistent_state() {
    // A fix that always regenerates would be a different bug, not a cure.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let (generated, key_stamp) = make_current_out_dir(dir.path(), &key);
    assert!(
        !needs_generate(Some(&key), &key_stamp, &generated),
        "must NOT regenerate when the outputs, headers, ABI, manifest and key stamp all agree"
    );
}

#[test]
fn test_needs_generate_true_without_a_key() {
    // No hasher or no CLI means the key is unknowable, and an unknowable key
    // can vouch for nothing.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let (generated, key_stamp) = make_current_out_dir(dir.path(), &key);
    assert!(
        needs_generate(None, &key_stamp, &generated),
        "an unknowable key must regenerate, even over a fully consistent dir"
    );
}

#[test]
fn test_needs_generate_true_when_the_key_stamp_disagrees() {
    if !hasher_available() {
        return;
    }
    let sha = "a".repeat(64);
    let key = generation_key(&sha, FIXTURE_CLI_VERSION);
    let cases: [(&str, Option<String>); 3] = [
        ("an absent key stamp", None),
        (
            "a key stamp from another grammar",
            Some(generation_key(&"b".repeat(64), FIXTURE_CLI_VERSION)),
        ),
        (
            "a key stamp from another CLI version",
            Some(generation_key(&sha, "tree-sitter 0.0.0 (other)")),
        ),
    ];
    for (label, recorded) in cases {
        let dir = tempfile::tempdir().unwrap();
        let (generated, key_stamp) = make_current_out_dir(dir.path(), &key);
        match recorded {
            None => std::fs::remove_file(&key_stamp).unwrap(),
            Some(text) => std::fs::write(&key_stamp, text).unwrap(),
        }
        assert!(
            needs_generate(Some(&key), &key_stamp, &generated),
            "must regenerate over {label}"
        );
    }
}

#[test]
fn test_needs_generate_true_when_a_generated_file_is_missing() {
    if !hasher_available() {
        return;
    }
    let key = fixture_key();
    for victim in EXPECTED_OUTPUTS.iter().chain(GENERATED_HEADERS) {
        let dir = tempfile::tempdir().unwrap();
        let (generated, key_stamp) = make_current_out_dir(dir.path(), &key);
        std::fs::remove_file(generated.join(victim)).unwrap();
        assert!(
            needs_generate(Some(&key), &key_stamp, &generated),
            "must regenerate when {victim} is missing"
        );
    }
}

#[test]
fn test_needs_generate_true_when_an_output_contradicts_its_manifest() {
    // The last byte, so parser.c's LANGUAGE_VERSION line stays intact and only
    // the manifest can see the change.
    if !hasher_available() {
        return;
    }
    let key = fixture_key();
    for victim in EXPECTED_OUTPUTS {
        let dir = tempfile::tempdir().unwrap();
        let (generated, key_stamp) = make_current_out_dir(dir.path(), &key);
        let path = generated.join(victim);
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 0x01;
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            needs_generate(Some(&key), &key_stamp, &generated),
            "must regenerate when {victim} differs from its manifest by one byte"
        );
    }
}

#[test]
fn test_needs_generate_true_when_the_outputs_manifest_is_absent() {
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let (generated, key_stamp) = make_current_out_dir(dir.path(), &key);
    std::fs::remove_file(outputs_stamp_of(&generated)).unwrap();
    assert!(
        needs_generate(Some(&key), &key_stamp, &generated),
        "an absent outputs manifest must force regeneration"
    );
}

#[test]
fn test_needs_generate_true_for_a_consistent_parser_at_another_abi() {
    // Task 9727's state: a parser generated before the ABI pin, behind a
    // manifest and key stamp that match it. Only the ABI check can see it.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let generated = make_populated_src_dir(dir.path());
    std::fs::write(
        generated.join("parser.c"),
        parser_c_declaring(&unpinned_abi()),
    )
    .unwrap();
    let key_stamp = dir.path().join(KEY_STAMP_NAME);
    write_generation_stamps(&generated, &key_stamp, &key);

    assert!(
        outputs_manifest_matches(&outputs_stamp_of(&generated), &generated),
        "precondition: the manifest must match the unpinned parser"
    );
    assert_eq!(
        std::fs::read_to_string(&key_stamp).unwrap(),
        key,
        "precondition: the key stamp must hold the current key"
    );
    assert!(
        needs_generate(Some(&key), &key_stamp, &generated),
        "a parser at ABI {} must regenerate, the runtime loads only ABI {}",
        unpinned_abi(),
        GENERATED_PARSER_ABI
    );
}

#[test]
fn test_generation_key_names_both_the_grammar_and_the_cli() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    assert_ne!(
        generation_key(&a, "tree-sitter 0.26.8"),
        generation_key(&a, "tree-sitter 0.27.0"),
        "a CLI upgrade alone must change the key"
    );
    assert_ne!(
        generation_key(&a, FIXTURE_CLI_VERSION),
        generation_key(&b, FIXTURE_CLI_VERSION),
        "a grammar change alone must change the key"
    );
}

#[test]
fn test_attested_grammar_sha_round_trips_a_written_key() {
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let generated = make_attested_generated_dir(dir.path());
    let key_stamp = dir.path().join(KEY_STAMP_NAME);
    let sha = "0123456789abcdef".repeat(4);
    write_generation_stamps(
        &generated,
        &key_stamp,
        &generation_key(&sha, FIXTURE_CLI_VERSION),
    );
    assert_eq!(
        attested_grammar_sha(&key_stamp),
        Some(sha),
        "the grammar sha must read back from a key stamp write_generation_stamps wrote"
    );
}

#[test]
fn test_attested_grammar_sha_is_none_for_an_absent_or_malformed_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let key_stamp = dir.path().join(KEY_STAMP_NAME);
    assert_eq!(
        attested_grammar_sha(&key_stamp),
        None,
        "an absent key stamp attests no grammar"
    );
    for malformed in [
        String::new(),
        format!("deadbeef\n{FIXTURE_CLI_VERSION}\n"),
        format!("{}\n{FIXTURE_CLI_VERSION}\n", "a".repeat(63)),
        format!("{}\n{FIXTURE_CLI_VERSION}\n", "a".repeat(65)),
        format!("{}g\n{FIXTURE_CLI_VERSION}\n", "a".repeat(63)),
        format!("{}\n{FIXTURE_CLI_VERSION}\n", "A".repeat(64)),
    ] {
        std::fs::write(&key_stamp, &malformed).unwrap();
        assert_eq!(
            attested_grammar_sha(&key_stamp),
            None,
            "a first line that is not 64 lowercase hex attests no grammar: {malformed:?}"
        );
    }
}

#[test]
fn test_parser_has_pinned_abi_reads_the_language_version_define() {
    let dir = tempfile::tempdir().unwrap();
    let parser = dir.path().join("parser.c");

    std::fs::write(&parser, parser_c_declaring(GENERATED_PARSER_ABI)).unwrap();
    assert!(
        parser_has_pinned_abi(&parser),
        "a parser declaring LANGUAGE_VERSION {GENERATED_PARSER_ABI} is pinned"
    );

    std::fs::write(&parser, parser_c_declaring(&unpinned_abi())).unwrap();
    assert!(
        !parser_has_pinned_abi(&parser),
        "a parser declaring LANGUAGE_VERSION {} is not pinned",
        unpinned_abi()
    );

    std::fs::write(&parser, "#include \"tree_sitter/parser.h\"\n").unwrap();
    assert!(
        !parser_has_pinned_abi(&parser),
        "a parser with no LANGUAGE_VERSION define is not pinned"
    );

    std::fs::remove_file(&parser).unwrap();
    assert!(
        !parser_has_pinned_abi(&parser),
        "an absent parser is not pinned"
    );
}

#[test]
fn test_generation_stamps_converge_in_one_write() {
    // Replaces the old "one bounded extra run" pin: build.rs watches nothing it
    // writes, so the stamps one regeneration writes must settle the predicate.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = fixture_key();
    let generated = make_pinned_generated_dir(dir.path());
    let key_stamp = dir.path().join(KEY_STAMP_NAME);

    assert!(
        needs_generate(Some(&key), &key_stamp, &generated),
        "round 1 must want a regeneration, or the rounds after it prove nothing"
    );

    write_generation_stamps(&generated, &key_stamp, &key);

    assert!(
        outputs_manifest_matches(&outputs_stamp_of(&generated), &generated),
        "the written .generated_outputs.stamp must describe the outputs on disk"
    );
    assert_eq!(
        std::fs::read_to_string(&key_stamp).unwrap(),
        key,
        "the key stamp must hold exactly the key"
    );
    for round in 2..=3 {
        assert!(
            !needs_generate(Some(&key), &key_stamp, &generated),
            "round {round}: the stamps one regeneration writes must settle the predicate"
        );
    }
}

#[test]
fn test_generation_stamps_write_neither_stamp_when_an_output_will_not_hash() {
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let generated = make_pinned_generated_dir(dir.path());
    std::fs::remove_file(generated.join("grammar.json")).unwrap();
    let key_stamp = dir.path().join(KEY_STAMP_NAME);

    write_generation_stamps(&generated, &key_stamp, &fixture_key());

    assert!(
        !key_stamp.exists(),
        "no key stamp may vouch for outputs the manifest could not describe"
    );
    assert!(
        !outputs_stamp_of(&generated).exists(),
        "no partial manifest may be written"
    );
}

#[test]
fn test_inputs_stamp_attests_the_grammar_the_key_names() {
    // The inputs stamp must describe the grammar that produced the compiled
    // parser.c, which is the one the key stamp names, not whatever grammar.js
    // holds when the stamp is written.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path();
    let generating_grammar = out_dir.join("generating-grammar.js");
    std::fs::write(
        &generating_grammar,
        b"module.exports = grammar({name: 'generated_from'});",
    )
    .unwrap();
    let recorded_sha = sha256_of_path(&generating_grammar).unwrap().unwrap();
    let crate_grammar = Path::new(BUILD_RS).with_file_name("grammar.js");
    assert_ne!(
        Some(recorded_sha.clone()),
        sha256_of_path(&crate_grammar).unwrap(),
        "precondition: the recorded sha must differ from the grammar.js on disk"
    );

    let key_stamp = out_dir.join(KEY_STAMP_NAME);
    std::fs::write(
        &key_stamp,
        generation_key(&recorded_sha, FIXTURE_CLI_VERSION),
    )
    .unwrap();
    let scanner_copy = out_dir.join(GENERATED_DIR).join("scanner.c");
    std::fs::create_dir_all(scanner_copy.parent().unwrap()).unwrap();
    std::fs::write(&scanner_copy, b"/* scanner fixture */\n").unwrap();
    let scanner_sha = sha256_of_path(&scanner_copy).unwrap().unwrap();

    write_inputs_stamp(out_dir, &key_stamp, &scanner_copy);

    assert_eq!(
        std::fs::read_to_string(out_dir.join(INPUTS_STAMP_NAME)).unwrap(),
        outputs_manifest_render(&[
            (recorded_sha, "grammar.js".to_string()),
            (scanner_sha, "src/scanner.c".to_string()),
        ]),
        "the inputs stamp must name the key stamp's grammar sha and the compiled scanner copy"
    );
}

#[test]
fn test_inputs_stamp_is_removed_without_a_key_stamp() {
    // Without a key stamp the compiled sources are unproven, and a stamp left
    // in place would attest bytes this build may not have compiled.
    if !hasher_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path();
    let stamp = out_dir.join(INPUTS_STAMP_NAME);
    std::fs::write(&stamp, "stale  grammar.js\n").unwrap();
    let scanner_copy = out_dir.join(GENERATED_DIR).join("scanner.c");
    std::fs::create_dir_all(scanner_copy.parent().unwrap()).unwrap();
    std::fs::write(&scanner_copy, b"/* scanner fixture */\n").unwrap();

    write_inputs_stamp(out_dir, &out_dir.join(KEY_STAMP_NAME), &scanner_copy);

    assert!(
        !stamp.exists(),
        "with no key stamp, no inputs stamp may remain"
    );
}

/// Write `body` to `path` with mode 0755.
#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Block until `path` can be exec'd.
///
/// A fork by another test thread while this one held the script open for
/// writing makes the first exec fail with ETXTBSY. Once one exec gets past
/// that, no writer descriptor remains anywhere.
#[cfg(unix)]
fn wait_until_executable(path: &Path) {
    for _ in 0..100 {
        match std::process::Command::new(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
    panic!("{} stayed busy (ETXTBSY)", path.display());
}

#[test]
#[cfg(unix)] // the exec bit find_on_path requires is a Unix permission
fn test_find_on_path_returns_the_first_executable_match() {
    if is_root() {
        eprintln!("skipping: test requires non-root user (root bypasses DAC permissions)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let name = "tree-sitter-fixture";
    let dirs: Vec<std::path::PathBuf> = ["empty", "not-executable", "first", "second"]
        .iter()
        .map(|d| dir.path().join(d))
        .collect();
    for d in &dirs {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(dirs[1].join(name), "#!/bin/sh\n").unwrap();
    write_executable(&dirs[2].join(name), "#!/bin/sh\n");
    write_executable(&dirs[3].join(name), "#!/bin/sh\n");

    let path_var = std::env::join_paths(&dirs).unwrap();
    assert_eq!(
        find_on_path(name, Some(path_var.as_os_str())),
        Some(dirs[2].join(name)),
        "the first executable match wins, past an empty dir and a non-executable file"
    );

    let only_non_executable = std::env::join_paths(&dirs[..2]).unwrap();
    assert_eq!(
        find_on_path(name, Some(only_non_executable.as_os_str())),
        None,
        "a same-named file without the exec bit is not a match"
    );
}

#[test]
fn test_find_on_path_is_none_when_the_name_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path_var = std::env::join_paths([dir.path()]).unwrap();
    assert_eq!(
        find_on_path("tree-sitter-fixture", Some(path_var.as_os_str())),
        None,
        "a name on no PATH dir is not found"
    );
    assert_eq!(
        find_on_path("tree-sitter-fixture", None),
        None,
        "an unset PATH finds nothing"
    );
}

#[test]
#[cfg(unix)] // the fixture CLIs are shell scripts made executable by mode bits
fn test_cli_version_returns_the_trimmed_version_line() {
    if is_root() {
        eprintln!("skipping: test requires non-root user (root bypasses DAC permissions)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("tree-sitter");
    write_executable(&cli, "#!/bin/sh\necho 'tree-sitter 9.9.9-fixture'\n");
    wait_until_executable(&cli);
    assert_eq!(
        cli_version(&cli),
        Some("tree-sitter 9.9.9-fixture".to_string()),
        "cli_version must return the CLI's trimmed --version stdout"
    );

    let failing = dir.path().join("failing");
    write_executable(
        &failing,
        "#!/bin/sh\necho 'tree-sitter 9.9.9-fixture'\nexit 1\n",
    );
    wait_until_executable(&failing);
    assert_eq!(
        cli_version(&failing),
        None,
        "a CLI whose --version fails reports no version"
    );

    assert_eq!(
        cli_version(&dir.path().join("absent")),
        None,
        "a nonexistent CLI reports no version"
    );
}
