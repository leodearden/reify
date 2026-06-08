//! Corpus-cleanliness guard: zero bare `: Scalar` type annotations (task δ).
//!
//! Signal: `: *Scalar([^<a-zA-Z]|$)` with `::Scalar` excluded.
//!
//! Walks:
//!   * `examples/**/*.ri`          — design example files
//!   * `crates/**/*.ri`            — standalone fixture .ri files
//!   * `crates/**/*.rs`            — inline .ri fixtures + doc-prose in Rust sources
//!   * `gui/test/**/*.ri`          — GUI fixture files
//!
//! This test is RED (~984 sites) until all δ migration steps complete.
//! It becomes compiler-redundant once γ adds `E_BARE_SCALAR`, but protects
//! the δ→γ window.
//!
//! Design decisions:
//!   * `::Scalar` (Rust enum paths `Type::Scalar` / `Value::Scalar`) are
//!     deliberately excluded — they are not type annotations and are not
//!     renamed by δ.
//!   * `Scalar<…>` and `Scalar` followed by a letter (e.g. `Scalars`) are
//!     not matched — they are either qualified or not the plain keyword.

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

/// Returns `true` when `line` contains a bare `: Scalar` type annotation that
/// must be migrated.
///
/// Matches: `: *Scalar([^<a-zA-Z]|$)` where the introducing `:` is **not**
/// preceded by another `:` (i.e., `::Scalar` Rust enum paths are excluded).
fn line_has_bare_scalar(line: &str) -> bool {
    let mut search_start = 0;
    while let Some(rel) = line[search_start..].find("Scalar") {
        let abs = search_start + rel;

        // 1. Check character immediately after "Scalar" — must not be `<` or ASCII letter.
        let after_ok = match line[abs + 6..].chars().next() {
            None => true,                                           // end of string / line
            Some(c) => c != '<' && !c.is_ascii_alphabetic(),
        };

        if after_ok {
            // 2. Scan backwards from `abs`, skipping spaces, to find the
            //    preceding non-space character.  It must be a single `:` that
            //    is NOT itself preceded by another `:`.
            let before = &line[..abs];
            let before_trimmed = before.trim_end_matches(' ');
            // ends_with(':') but NOT ends_with("::") → bare colon annotation
            if before_trimmed.ends_with(':') && !before_trimmed.ends_with("::") {
                return true;
            }
        }

        search_start = abs + 6;
    }
    false
}

#[test]
fn corpus_has_zero_bare_scalar_annotations() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();

    // A. examples/**/*.ri — design example files
    collect_files(&root.join("examples"), "ri", &mut files);

    // B + C + D. crates/**/*.ri (fixture .ri files) + crates/**/*.rs (inline fixtures, doc-prose)
    collect_files(&root.join("crates"), "ri", &mut files);
    collect_files(&root.join("crates"), "rs", &mut files);

    // GUI fixture .ri files
    let gui_fixtures = root.join("gui").join("test");
    collect_files(&gui_fixtures, "ri", &mut files);

    // Deduplicate: the crates/ walk can't overlap with examples/ or gui/, but
    // sort + dedup keeps the list tidy.
    files.sort();
    files.dedup();

    // Exclude this guard-test file itself — it contains `: Scalar` in its own
    // comments, strings, and unit-test literals.  Scanning it would create
    // self-referential false positives that prevent the test from ever going GREEN.
    files.retain(|p| {
        p.file_name().and_then(|f| f.to_str()) != Some("corpus_no_bare_scalar.rs")
    });

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
        "Found {} bare `: Scalar` annotation(s). Migrate each to `: Length`:\n\n{}",
        violations.len(),
        violations.join("\n")
    );
}

// ── Unit tests for the detection predicate ─────────────────────────────────

#[cfg(test)]
mod predicate_tests {
    use super::line_has_bare_scalar;

    // Should match (violations)
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
        assert!(line_has_bare_scalar("    fn area(w: Scalar, h: Scalar) -> Scalar"));
    }

    #[test]
    fn detects_bare_scalar_in_inline_ri_string() {
        assert!(line_has_bare_scalar(r#"    let src = "param w: Scalar = 50mm";"#));
    }

    // Should NOT match (correctly excluded)
    #[test]
    fn excludes_double_colon_scalar() {
        assert!(!line_has_bare_scalar("    let t = Type::Scalar { dimension: LENGTH };"));
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
    fn excludes_scalar_followed_by_letter() {
        assert!(!line_has_bare_scalar("    // Scalars and tensors"));
    }

    #[test]
    fn excludes_return_scalar() {
        // `-> Scalar` is NOT a colon-introduced annotation (δ does not migrate it)
        assert!(!line_has_bare_scalar("    fn area(w: Length) -> Scalar"));
    }

    #[test]
    fn excludes_comment_only_double_colon() {
        assert!(!line_has_bare_scalar("    // see Type::Scalar for details"));
    }
}
