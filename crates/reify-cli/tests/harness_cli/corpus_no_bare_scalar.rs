//! Corpus-cleanliness guard: zero bare `: Scalar` type annotations and bare
//! `-> Scalar` return codomains (tasks δ + δ-completion).
//!
//! Signal: `: *Scalar([^<a-zA-Z]|$)` (annotation) or `-> Scalar([^<a-zA-Z]|$)`
//! (codomain), with pure-comment lines and `::Scalar` excluded.
//!
//! Walks:
//!   * `examples/**/*.ri`          — design example files
//!   * `crates/**/*.ri`            — standalone fixture .ri files
//!   * `crates/**/*.rs`            — inline .ri fixtures in Rust sources
//!     (comment/doc-prose lines are excluded — see predicate)
//!   * `gui/src-tauri/**/*.rs`     — GUI inline DSL test sources
//!   * `gui/test/**/*.ri`          — GUI fixture files
//!
//! Excluded from scan (parse-only, pin literal "Scalar", never type-resolve):
//!   * `crates/reify-syntax/tests/`
//!   * `crates/reify-ast/tests/`
//!
//! This test is GREEN (δ migration complete). It becomes compiler-redundant
//! once γ adds `E_BARE_SCALAR`, but protects the δ→γ window as a regression
//! guard.
//!
//! Design decisions:
//!   * `::Scalar` (Rust enum paths `Type::Scalar` / `Value::Scalar`) are
//!     deliberately excluded — they are not type annotations and are not
//!     renamed by δ.
//!   * `Scalar<…>` and `Scalar` followed by a letter (e.g. `Scalars`) are
//!     not matched — they are either qualified or not the plain keyword.
//!   * Pure comment lines (trimmed starts with `//`) are skipped — doc-prose
//!     that quotes `-> Scalar` or `: Scalar` in comments must not be flagged.
//!   * A Rust `{:#?}` Debug struct-field opener — a whole line of the exact
//!     shape `<ident>: Scalar {` — is excluded. `#[derive(Debug)]` renders the
//!     `reify_ir::Value::Scalar` ENUM VARIANT *unqualified*, eliding the
//!     `Value::` prefix the `::Scalar` rule above keys on, so a snapshot golden
//!     of a dimensioned IR field reads as `width: Scalar {`. This is the SAME
//!     carve-out with the same rationale — a Rust enum variant name, never a
//!     DSL type annotation. The `-> Scalar {` codomain arm is deliberately
//!     untouched: `field def t : Point3 -> Scalar { … }` is real DSL and stays
//!     a violation.

use std::path::{Path, PathBuf};

/// Resolve the workspace root from CARGO_MANIFEST_DIR.
///
/// `reify-cli` lives at `<root>/crates/reify-cli`, so the workspace root is
/// two levels up.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root must be accessible")
}

/// Walk `dir` recursively, appending every file whose extension equals `ext`
/// to `out`.  Silently skips unreadable entries.
fn collect_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

/// Strip a trailing `//` line comment from `line`, returning the portion before
/// the first comment marker.  `://` (URL schemes that may appear in string
/// literals) are preserved — only `//` not immediately preceded by `:` is
/// treated as a comment start.
///
/// Limitation: `/* … */` block comments are **not** stripped.  No such block
/// comment with a bare-`Scalar` mention exists in the scanned corpus today.
fn strip_trailing_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Preserve `://` (URL scheme) inside string literals.
            if i == 0 || bytes[i - 1] != b':' {
                return &line[..i];
            }
        }
        i += 1;
    }
    line
}

/// Returns `true` when the `Scalar` occurrence at byte offset `abs` in `line`
/// is the Rust pretty-`Debug` (`{:#?}`) rendering of the `reify_ir::Value::Scalar`
/// ENUM VARIANT as a struct field — i.e. the whole line is exactly
/// `<indent><ident>: Scalar {`.
///
/// `#[derive(Debug)]` prints a struct-like variant UNQUALIFIED, so the `Value::`
/// prefix that [`line_has_bare_scalar`]'s `::Scalar` rule keys on is elided:
/// a `{:#?}` golden of a LENGTH-dimensioned IR field (task 5743's R7 raw-`Value`
/// chokepoint) reads as `width: Scalar {`. Those goldens are Rust type names in
/// a snapshot string, never DSL type annotations, so they are excluded for the
/// same reason `Value::Scalar` already is.
///
/// The shape is matched WHOLE-LINE and narrowly on purpose:
///   * everything after `Scalar` must be exactly ` {` — Rust pretty-Debug puts
///     nothing else on a struct-variant opener line;
///   * everything before the `: ` must be leading whitespace plus one plain
///     Rust identifier — so DSL forms never match. `param w: Scalar = 10mm`
///     fails on both counts, and `structure def X : Scalar {` (the only DSL
///     shape that *is* `: <Type> {`, a structure supertype) fails because
///     `structure def X` is not a single identifier.
fn is_rust_debug_scalar_field(line: &str, abs: usize) -> bool {
    // Tail must be exactly ` {` (trailing whitespace tolerated).
    if line[abs + 6..].trim_end() != " {" {
        return false;
    }
    // Head must be `<whitespace><ident>: `.
    let Some(head) = line[..abs].strip_suffix(": ") else {
        return false;
    };
    let ident = head.trim_start_matches([' ', '\t']);
    !ident.is_empty()
        && ident.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Returns `true` when `line` contains a bare `: Scalar` type annotation or a
/// bare `-> Scalar` return codomain that must be migrated.
///
/// Matches:
///   * `: *Scalar([^<a-zA-Z]|$)` where the introducing `:` is **not**
///     preceded by another `:` (i.e., `::Scalar` Rust enum paths excluded).
///   * `-> Scalar([^<a-zA-Z]|$)` — bare return codomain.
///
/// Pure comment lines (trimmed starts with `//`) are always skipped, and any
/// trailing line comment is stripped before scanning — only real source /
/// inline-DSL string content is examined.
fn line_has_bare_scalar(line: &str) -> bool {
    // Skip pure comment lines — doc-prose mentioning `-> Scalar` or `: Scalar`
    // in comments must not be treated as migration violations.
    if line.trim_start().starts_with("//") {
        return false;
    }

    // Strip any trailing line comment before scanning.  This prevents a
    // migrated line like `-> Length { } // was -> Scalar` from being falsely
    // flagged due to the `-> Scalar` mention in the comment.
    let line = strip_trailing_line_comment(line);

    let mut search_start = 0;
    while let Some(rel) = line[search_start..].find("Scalar") {
        let abs = search_start + rel;

        // 1. Check character immediately after "Scalar" — must not be `<` or ASCII letter.
        let after_ok = match line[abs + 6..].chars().next() {
            None => true, // end of string / line
            Some(c) => c != '<' && !c.is_ascii_alphabetic(),
        };

        // 1b. A Rust `{:#?}` Debug struct-field opener (`width: Scalar {`) is
        //     the unqualified rendering of the `Value::Scalar` enum variant —
        //     excluded for the same reason `::Scalar` is (see the helper).
        if after_ok && !is_rust_debug_scalar_field(line, abs) {
            // 2. Scan backwards from `abs`, skipping spaces, to find the
            //    preceding non-space character.  It must be:
            //    (a) a single `:` NOT preceded by another `:` → bare annotation, OR
            //    (b) `->` → bare return codomain.
            let before = &line[..abs];
            let before_trimmed = before.trim_end_matches(' ');
            // (a) annotation: ends_with(':') but NOT ends_with("::") → bare colon annotation
            if before_trimmed.ends_with(':') && !before_trimmed.ends_with("::") {
                return true;
            }
            // (b) codomain: ends_with("->") → bare return type
            if before_trimmed.ends_with("->") {
                return true;
            }
        }

        search_start = abs + 6;
    }
    false
}

#[test]
fn corpus_has_zero_bare_scalar() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();

    // A. examples/**/*.ri — design example files
    collect_files(&root.join("examples"), "ri", &mut files);

    // B + C + D. crates/**/*.ri (fixture .ri files) + crates/**/*.rs (inline fixtures, doc-prose)
    collect_files(&root.join("crates"), "ri", &mut files);
    collect_files(&root.join("crates"), "rs", &mut files);

    // E. gui/src-tauri/**/*.rs — all GUI src-tauri Rust sources (production + inline-DSL
    //    tests). The recursive walk intentionally covers production files (engine.rs,
    //    types.rs, …) as well as the inline-DSL test fixtures; the wider coverage is
    //    beneficial — any bare `Scalar` reaching type resolution from GUI code is caught
    //    here too. (δ-completion: δ's guard never scanned this tree.)
    collect_files(&root.join("gui").join("src-tauri"), "rs", &mut files);

    // F. GUI fixture .ri files
    let gui_fixtures = root.join("gui").join("test");
    collect_files(&gui_fixtures, "ri", &mut files);

    // Deduplicate: the crates/ walk can't overlap with examples/ or gui/, but
    // sort + dedup keeps the list tidy.
    files.sort();
    files.dedup();

    // Exclude this guard-test file itself — it contains `: Scalar` and `-> Scalar`
    // in its own comments, strings, and unit-test literals.  Scanning it would create
    // self-referential false positives that prevent the test from ever going GREEN.
    files.retain(|p| p.file_name().and_then(|f| f.to_str()) != Some("corpus_no_bare_scalar.rs"));

    // Exclude parse-only test directories — they pin the LITERAL PARSED name
    // "Scalar" (never reach type resolution, can never be E_BARE_SCALAR violators).
    //   * crates/reify-syntax/tests/ — field_tests.rs:30,64 assert codomain_type.to_string()=="Scalar"
    //   * crates/reify-ast/tests/   — api_surface.rs:70 asserts name=="Scalar"
    //
    // Accepted blind spot: the exclusion is directory-level, not file-level.
    // A new test file added to either directory would also be excluded from the
    // scan.  This is intentional: every test in reify-syntax/tests/ and
    // reify-ast/tests/ is parse-only by design — none reach type resolution,
    // so none can ever be E_BARE_SCALAR (γ) violators.  The carve-out matches
    // the invariant the directories enforce, not just the two current files.
    let syntax_tests = root.join("crates").join("reify-syntax").join("tests");
    let ast_tests = root.join("crates").join("reify-ast").join("tests");
    files.retain(|p| !p.starts_with(&syntax_tests) && !p.starts_with(&ast_tests));

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = path.strip_prefix(&root).unwrap_or(path);
        for (line_idx, line) in content.lines().enumerate() {
            if line_has_bare_scalar(line) {
                violations.push(format!(
                    "{}:{}: {}",
                    rel.display(),
                    line_idx + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found {} bare `Scalar` annotation(s) or codomain(s). \
         Migrate each `: Scalar` -> `: Length` and `-> Scalar` -> `-> Length`:\n\n{}",
        violations.len(),
        violations.join("\n")
    );
}

// ── Unit tests for the detection predicate ─────────────────────────────────

#[cfg(test)]
mod predicate_tests {
    use super::line_has_bare_scalar;

    // Should match (violations) — annotation cases
    #[test]
    fn detects_bare_scalar_with_space() {
        assert!(line_has_bare_scalar("    param width: Scalar = 10mm"));
    }

    #[test]
    fn detects_bare_scalar_no_space() {
        assert!(line_has_bare_scalar("    param width:Scalar = 10mm"));
    }

    #[test]
    fn detects_bare_scalar_at_end_of_line() {
        assert!(line_has_bare_scalar("    fn foo(x: Scalar"));
    }

    #[test]
    fn detects_bare_scalar_followed_by_comma() {
        assert!(line_has_bare_scalar("    fn foo(x: Scalar, y: Scalar)"));
    }

    #[test]
    fn detects_bare_scalar_followed_by_paren() {
        assert!(line_has_bare_scalar(
            "    fn area(w: Scalar, h: Scalar) -> Scalar"
        ));
    }

    #[test]
    fn detects_bare_scalar_in_inline_ri_string() {
        assert!(line_has_bare_scalar(
            r#"    let src = "param w: Scalar = 50mm";"#
        ));
    }

    // Should match (violations) — codomain cases
    #[test]
    fn detects_return_scalar() {
        // `-> Scalar` IS a bare return codomain (δ-completion migrates it)
        assert!(line_has_bare_scalar("    fn area(w: Length) -> Scalar"));
    }

    #[test]
    fn detects_return_scalar_with_brace() {
        assert!(line_has_bare_scalar(
            "    field def temp : Point3 -> Scalar { 1.0m }"
        ));
    }

    #[test]
    fn detects_return_scalar_at_end_of_line() {
        assert!(line_has_bare_scalar("    fn foo() -> Scalar"));
    }

    // Should NOT match (correctly excluded)
    #[test]
    fn excludes_double_colon_scalar() {
        assert!(!line_has_bare_scalar(
            "    let t = Type::Scalar { dimension: LENGTH };"
        ));
    }

    #[test]
    fn excludes_value_double_colon_scalar() {
        assert!(!line_has_bare_scalar("    Value::Scalar(v)"));
    }

    #[test]
    fn excludes_scalar_with_angle_bracket() {
        assert!(!line_has_bare_scalar("    param x: Scalar<Length> = 10mm"));
    }

    #[test]
    fn excludes_return_scalar_parameterized() {
        // `-> Scalar<Q>` is NOT bare — parameterized, not a migration target
        assert!(!line_has_bare_scalar("    fn foo() -> Scalar<Length>"));
    }

    #[test]
    fn excludes_scalar_followed_by_letter() {
        assert!(!line_has_bare_scalar("    // Scalars and tensors"));
    }

    #[test]
    fn excludes_comment_only_double_colon() {
        assert!(!line_has_bare_scalar("    // see Type::Scalar for details"));
    }

    #[test]
    fn excludes_comment_line_with_return_scalar() {
        // Pure comment lines are skipped entirely
        assert!(!line_has_bare_scalar(
            "    // field def area(w: Length) -> Scalar"
        ));
    }

    #[test]
    fn excludes_comment_line_with_annotation_scalar() {
        // Pure comment lines are skipped even for annotation form
        assert!(!line_has_bare_scalar("    // param x: Scalar = 10mm"));
    }

    // Trailing-comment stripping — non-comment lines whose trailing `// ...`
    // mentions a bare Scalar must NOT be flagged (the code itself is migrated).
    #[test]
    fn excludes_trailing_comment_with_return_scalar() {
        assert!(!line_has_bare_scalar(
            "    field def t : Point3 -> Length {} // was -> Scalar"
        ));
    }

    #[test]
    fn excludes_trailing_comment_with_annotation_scalar() {
        assert!(!line_has_bare_scalar(
            "    param width: Length = 10mm // was: Scalar"
        ));
    }

    // Rust `{:#?}` Debug goldens — `#[derive(Debug)]` renders the
    // `Value::Scalar` enum variant unqualified, so a snapshot of a
    // LENGTH-dimensioned IR field reads as `<field>: Scalar {`.
    #[test]
    fn excludes_rust_debug_scalar_struct_field() {
        assert!(!line_has_bare_scalar("        width: Scalar {"));
    }

    #[test]
    fn excludes_rust_debug_scalar_struct_field_underscore_ident() {
        assert!(!line_has_bare_scalar("        outer_r: Scalar {"));
    }

    // …and the carve-out must stay narrow: everything below is still a
    // violation.
    #[test]
    fn detects_bare_scalar_annotation_despite_trailing_brace_rule() {
        // A DSL param annotation has more than an identifier before the `:`
        // and does not end in ` {` — unaffected by the Debug carve-out.
        assert!(line_has_bare_scalar("    param width: Scalar = 10mm"));
    }

    #[test]
    fn detects_dsl_structure_supertype_scalar_with_brace() {
        // `structure def X : Scalar {` is the one DSL shape that is `: <Type> {`;
        // `structure def X` is not a single identifier, so it still matches.
        assert!(line_has_bare_scalar("structure def OddRule : Scalar {"));
    }

    #[test]
    fn detects_return_scalar_with_brace_is_not_debug_excluded() {
        // The codomain arm is untouched by the Debug carve-out.
        assert!(line_has_bare_scalar(
            "    field def temp : Point3 -> Scalar {"
        ));
    }

    #[test]
    fn preserves_scalar_before_url_double_slash() {
        // `://` in a string literal must not be mistaken for a comment start;
        // bare Scalar appearing later on the same line must still be detected.
        assert!(line_has_bare_scalar(
            r#"    let _u = "https://x.com"; let src = "param x: Scalar";"#
        ));
    }
}
