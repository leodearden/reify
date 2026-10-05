//! Which calls a piece of text makes, read as TEXT: the per-name call-site
//! scan, and the distinct-called-names scan confirmed through it.
//!
//! Text scans, so they read a fence body, a chunk's markdown prose and an
//! example `.ri` file alike, whether or not the text parses.

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
fn calls_sees_a_call_and_the_call_nested_in_its_arguments() {
    let text = "let b = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)";
    assert!(calls(text, "translate"), "the outer call is a call");
    assert!(
        calls(text, "box"),
        "a call nested in another's argument list is a call in its own right"
    );
}

#[test]
fn calls_reads_an_empty_argument_list_as_a_call() {
    assert!(calls("let m0 = mechanism()", "mechanism"));
}

#[test]
fn calls_skips_an_identifier_prefixed_match() {
    assert!(
        !calls("let x = xmin_clearance(s, id_a, id_b)", "min_clearance"),
        "`min_clearance(` must not be harvested out of a longer identifier"
    );
}

#[test]
fn calls_skips_a_call_form_whose_parens_never_balance() {
    assert!(
        !calls("min_clearance(s, id_a,", "min_clearance"),
        "an unbalanced call form is skipped rather than guessed at"
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
