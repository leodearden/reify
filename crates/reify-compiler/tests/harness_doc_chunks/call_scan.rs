//! Which calls a piece of Reify text makes, read as TEXT: the comment stripper
//! every call scan runs behind, the per-name call-site scan, and the
//! distinct-called-names scan confirmed through it.
//!
//! Text scans, so they read a fence body, a chunk's markdown prose and an
//! example `.ri` file alike, whether or not the text parses.

/// `src` with Reify comments removed. Every other byte — newlines included — is
/// left exactly where it was, so a stripped fence still reads like the original
/// in a panic message.
///
/// WHY THIS EXISTS. [`call_sites`] is a text scan, so without it a call form
/// written only in a `//` comment counts as a real call. That was live, not
/// hypothetical: geometry.md's FORM A fence carries the line
/// `// MUST be let-bound. Writing `constraint min_clearance(s, id_a, id_b) > 2mm``,
/// whose 3-arg `min_clearance(` is exactly the documented arity — so deleting the
/// fence's REAL `let clr = min_clearance(s, id_a, id_b)` left both of
/// `geometry_chunk_smoke.rs`'s `geometry_reify_fences_call_every_worked_example_form`
/// and `oracle_signature_arities_match_the_compiling_fences` green while
/// their panic text claimed the form was "compile-verified" / "exercised by a
/// compiling fence". A commented-out call is not a call.
///
/// NOT AN AST WALK — yet. `doc_forms::call_forms` (`pub(crate)`) already extracts
/// `(name, arity)` from the real parser, which would close this hole for free AND
/// handle nesting exactly; swapping this scan onto it is task #8036, which first
/// extends that walk to `constraint` members. Copying its ~120-line exhaustive
/// `ExprKind` match here instead would add a second call extractor to this
/// binary. Until then this stripper plus the unit tests at the bottom of this
/// file are the guard.
///
/// Handles both comment forms the grammar defines (`tree-sitter-reify/grammar.js`
/// `line_comment` / `block_comment`) and does not strip inside a double-quoted
/// string. `://` is deliberately NOT a comment start, so the same helper is safe
/// on the chunk's markdown prose, where a URL would otherwise truncate its line.
/// A mis-tracked string can only cause a comment to survive, never content to be
/// dropped — i.e. it degrades to the un-stripped behaviour, never past it.
pub(crate) fn strip_reify_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                }
                '"' => in_string = false,
                // An unterminated literal ends at the line break rather than
                // swallowing the rest of the input.
                '\n' => in_string = false,
                _ => {}
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            // `//` to end of line — but not the `//` in a `scheme://` URL.
            '/' if chars.peek() == Some(&'/') && !out.ends_with(':') => {
                chars.next();
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            // `/* … */`, newlines preserved so line structure survives.
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev_star = false;
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                    }
                    if prev_star && n == '/' {
                        break;
                    }
                    prev_star = n == '*';
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Every call to `name` in `text`, as `(arity, byte offset just past the closing
/// paren)`, in document order.
///
/// `text` MUST already be comment-free — pass it through
/// [`strip_reify_comments`] first. This function cannot tell a call from a
/// mention of one.
///
/// Arity is TOP-LEVEL commas + 1 over the balanced argument list, so a nested
/// call (`translate(box(a, b, c), …)`) contributes ONE argument and an empty list
/// is arity 0. Two things are skipped rather than guessed at: a `name(` whose
/// parens never balance (a call form wrapped across a markdown line), and a match
/// preceded by an identifier character, so `min_clearance(` is not harvested out
/// of a hypothetical `xmin_clearance(`.
pub(crate) fn call_sites(text: &str, name: &str) -> Vec<(usize, usize)> {
    let needle = format!("{name}(");
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while let Some(rel) = text[cursor..].find(&needle) {
        let ident_start = cursor + rel;
        let open = ident_start + needle.len() - 1; // byte index of the `(`
        cursor = open + 1;

        if text[..ident_start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }

        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in text[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    // Cannot underflow: this scan starts AT the `(`, so `depth`
                    // is already >= 1 by the time any `)` is reached. Saturating
                    // here would be WRONG, not safer — it would make the
                    // `== 0` test fire on a stray `)` and report a short arity.
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { continue };

        let inner = &text[open + 1..close];
        if inner.trim().is_empty() {
            out.push((0, close + 1));
            continue;
        }
        // Only `(`/`[` nest here. `<`/`>` are deliberately NOT treated as
        // brackets: they appear in this chunk as comparisons far more often than
        // as type parameters, and an unbalanced `>` would silently swallow the
        // commas after it.
        let mut depth = 0usize;
        let mut arity = 1usize;
        for c in inner.chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => arity += 1,
                _ => {}
            }
        }
        out.push((arity, close + 1));
    }
    out
}

/// Every DISTINCT identifier CALLED as `name(` in `text`, in document order.
///
/// `text` MUST already be comment-free — pass it through
/// [`strip_reify_comments`] first.
///
/// Complements [`call_sites`], which answers "at what arities is THIS name
/// called". This answers "which names are called AT ALL" — the direction a
/// phantom-signature check needs, because a phantom name is by definition one
/// nobody thought to ask about. `min_clearance(a, b)` was found by asking the
/// first question; `rotate(geo, axis, angle)` and `translate(geo, vector)`
/// (tasks #5347 / #5364) could only have been found by asking this one.
///
/// Each candidate is CONFIRMED through [`call_sites`] rather than trusted, so
/// the two scanners cannot disagree about what a call is: a `name(` whose parens
/// never balance (a call form wrapped across a markdown line) is skipped here by
/// exactly the rule that skips it there. A run starting with a digit is a
/// numeric literal juxtaposed with a paren, never a call.
pub(crate) fn called_names(text: &str) -> Vec<String> {
    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();

    for (i, &c) in chars.iter().enumerate() {
        if c != '(' {
            continue;
        }
        let mut start = i;
        while start > 0 && is_ident(chars[start - 1]) {
            start -= 1;
        }
        if start == i {
            continue;
        }
        let name: String = chars[start..i].iter().collect();
        if name.starts_with(|c: char| c.is_ascii_digit()) || out.contains(&name) {
            continue;
        }
        if call_sites(text, &name).is_empty() {
            continue;
        }
        out.push(name);
    }
    out
}

// --- Scanner unit tests ------------------------------------------------------
//
// Every doc↔fence and phantom-name assertion in the chunk modules is downstream
// of one of these three scanners, so they are pinned directly here rather than
// only through the chunks.
//
// THE FAILURE THESE CLOSE IS SELF-CONCEALING. `called_names` is the sole
// extraction path for every chunk module's phantom-name gate, and each of those
// gates draws its anti-vacuity floors and sentinels from the same scanner's OWN
// output. A regression that made a scanner over-skip would weaken the gate while
// leaving every floor and sentinel satisfied, because both sides of the
// comparison would shrink together. Only a direct test over a known input can
// see that.

#[test]
fn call_sites_counts_a_nested_call_as_one_argument() {
    let arities: Vec<usize> = call_sites(
        "let b = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)",
        "translate",
    )
    .into_iter()
    .map(|(arity, _)| arity)
    .collect();
    assert_eq!(
        arities,
        vec![4],
        "the nested `box(...)` must contribute ONE argument, not its own three"
    );
}

#[test]
fn call_sites_reads_an_empty_argument_list_as_arity_zero() {
    let arities: Vec<usize> = call_sites("let m0 = mechanism()", "mechanism")
        .into_iter()
        .map(|(arity, _)| arity)
        .collect();
    assert_eq!(arities, vec![0]);
}

#[test]
fn call_sites_skips_an_identifier_prefixed_match() {
    assert!(
        call_sites("let x = xmin_clearance(s, id_a, id_b)", "min_clearance").is_empty(),
        "`min_clearance(` must not be harvested out of a longer identifier"
    );
}

#[test]
fn call_sites_skips_a_call_form_whose_parens_never_balance() {
    assert!(
        call_sites("min_clearance(s, id_a,", "min_clearance").is_empty(),
        "an unbalanced call form is skipped rather than guessed at"
    );
}

#[test]
fn call_sites_offset_lands_just_past_the_closing_paren() {
    // This is the offset `geometry_chunk_smoke.rs`'s `documented_signature_arities`
    // uses to find the `->`.
    let src = "`distance(a, b) -> Length` (2-arg)";
    let sites = call_sites(src, "distance");
    assert_eq!(sites.len(), 1);
    let (arity, after) = sites[0];
    assert_eq!(arity, 2);
    assert!(
        src[after..].trim_start().starts_with("-> Length"),
        "expected the return annotation just past the closing paren, got {:?}",
        &src[after..]
    );
}

#[test]
fn call_sites_does_not_see_a_call_that_only_appears_in_a_comment() {
    // geometry.md's FORM A fence, reduced to the two lines that matter: the
    // annotation names the 3-arg form, the code calls the 2-arg one.
    let fence = "// Writing `constraint min_clearance(s, id_a, id_b) > 2mm` inline is wrong.\n\
                 let clr = min_clearance(s, id_a)";

    let raw: Vec<usize> = call_sites(fence, "min_clearance")
        .into_iter()
        .map(|(arity, _)| arity)
        .collect();
    assert_eq!(
        raw,
        vec![3, 2],
        "a RAW scan sees the commented form too — this is the hole `strip_reify_comments` closes"
    );

    let code: Vec<usize> = call_sites(&strip_reify_comments(fence), "min_clearance")
        .into_iter()
        .map(|(arity, _)| arity)
        .collect();
    assert_eq!(
        code,
        vec![2],
        "a comment-free scan must see only the call the compiler actually gets"
    );
}

#[test]
fn strip_reify_comments_leaves_ordinary_source_untouched() {
    let src = "structure def S {\n    let g = box(1mm, 2mm, 3mm)\n}";
    assert_eq!(strip_reify_comments(src), src);
}

#[test]
fn strip_reify_comments_keeps_a_url_intact() {
    // The same helper runs over the chunk's markdown prose, where `//` after a
    // scheme is not a comment.
    let src = "see https://example.test/clearance for more";
    assert_eq!(strip_reify_comments(src), src);
}

#[test]
fn strip_reify_comments_leaves_a_double_slash_inside_a_string_literal() {
    let src = r#"let m1 = body(m0, "a//b", fixed()) // drop me"#;
    assert_eq!(
        strip_reify_comments(src),
        r#"let m1 = body(m0, "a//b", fixed()) "#
    );
}

#[test]
fn strip_reify_comments_removes_a_block_comment_and_preserves_line_count() {
    let src = "let a = box(1mm, 1mm, 1mm)\n/* two\n   lines */\nlet b = sphere(1mm)";
    let out = strip_reify_comments(src);
    assert_eq!(
        out.lines().count(),
        src.lines().count(),
        "line structure must survive so panic messages still line up with the chunk"
    );
    assert!(
        !out.contains("two"),
        "block-comment body must be gone: {out:?}"
    );
    assert!(
        out.contains("sphere(1mm)"),
        "code after the comment must survive"
    );
}

/// A digit-prefixed run juxtaposed with `(` is a numeric literal, not a call.
///
/// The discriminator `called_names` documents, asserted directly. Without the
/// skip, `2(x + 1)` would feed `2` to `registry_family` and every registry gate
/// downstream would fail on a name no author ever wrote.
#[test]
fn called_names_skips_a_numeric_literal_juxtaposed_with_a_paren() {
    assert_eq!(
        called_names("let scaled = 2(x) + box(1mm, 1mm, 1mm)"),
        vec!["box".to_string()],
        "`2(` is a literal juxtaposed with a paren; only `box` is a call"
    );
}

/// A `name(` whose parens never balance is skipped, matching `call_sites`.
///
/// The two scanners CONFIRM each other by construction — `called_names` runs
/// every candidate back through `call_sites` — and this pins that the
/// confirmation actually discriminates. A call form wrapped across a markdown
/// line is the live shape this protects against: half a call is not a call, and
/// counting it would let a fence "demonstrate" a form it never compiled.
///
/// The skip is PER NAME, not per line, which is the behaviour the assertion
/// below fixes in place: in `translate(box(1mm, 1mm, 1mm),` the OUTER
/// `translate(` never closes and is dropped, while the INNER `box(…)` closes on
/// its own and is kept. That is the right call — `box` really is demonstrated
/// here — and it is worth pinning precisely because the coarser "drop the whole
/// unbalanced line" reading is the one a reader assumes.
#[test]
fn called_names_skips_a_call_whose_parens_never_balance() {
    assert_eq!(
        called_names("let wrapped = translate(box(1mm, 1mm, 1mm),"),
        vec!["box".to_string()],
        "`translate(` never closes so it is not a call; the nested `box(…)` does, so it is"
    );
    // Control: the same text, closed, yields BOTH in document order — so the
    // assertion above is about `translate` being unbalanced, not about the
    // scanner being unable to see an outer call at all.
    assert_eq!(
        called_names("let ok = translate(box(1mm, 1mm, 1mm), 0mm, 0mm, 0mm)"),
        vec!["translate".to_string(), "box".to_string()]
    );
}

/// Repeated calls collapse to ONE entry, and the order is the order of first
/// appearance.
///
/// Both halves matter to the callers: the registry loops assert per DISTINCT
/// name, and every anti-vacuity floor over it counts `names.len()`, so a scanner
/// that stopped deduping would inflate a floor into passing on one repeated
/// constructor.
#[test]
fn called_names_dedups_and_preserves_document_order() {
    assert_eq!(
        called_names("polygon(0mm, 0mm) ; nurbs(1, 2) ; polygon(1mm, 1mm) ; nurbs(3, 4)"),
        vec!["polygon".to_string(), "nurbs".to_string()]
    );
}

/// A call appearing ONLY inside a `//` comment is absent once the input is
/// comment-stripped.
///
/// `called_names` documents "`text` MUST already be comment-free" as a
/// PRECONDITION, not a behaviour — it does no stripping of its own. This pins
/// the contract from the caller's side, which is how every chunk module uses it:
/// `called_names(&strip_reify_comments(&section))`. The hazard is live, not
/// hypothetical — geometry.md's FORM A fence carries a commented call at exactly
/// the documented arity (see `strip_reify_comments`), and a commented-out call
/// is not a call.
#[test]
fn called_names_does_not_see_a_call_that_only_appears_in_a_comment() {
    let src = "// let old = helix(10mm, 2mm, 50mm)\nlet spine = interp(0mm, 0mm, 0mm)";
    assert_eq!(
        called_names(&strip_reify_comments(src)),
        vec!["interp".to_string()],
        "the commented `helix(` must not count once the input is stripped"
    );
    // Control: UNSTRIPPED, the scanner does see it — so the assertion above is
    // about the precondition being honoured, not about the input being inert.
    assert!(called_names(src).contains(&"helix".to_string()));
}
