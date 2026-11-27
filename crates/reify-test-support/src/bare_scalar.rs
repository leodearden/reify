//! The predicate of a bare `: Scalar` type annotation or a bare `-> Scalar`
//! return codomain, shared by every guard that sweeps `.ri` sources.
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
pub fn line_has_bare_scalar(line: &str) -> bool {
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

#[cfg(test)]
mod tests {
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
