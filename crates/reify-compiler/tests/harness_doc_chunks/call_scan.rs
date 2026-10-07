//! Which names a chunk's markdown PROSE calls, read as TEXT because prose has
//! no parser: the per-name presence predicate [`calls`], and the
//! distinct-called-names scan [`called_names`] confirmed through it.
//!
//! Reify SOURCE — a chunk's ```` ```reify ```` fences, an example `.ri` file, a
//! signature fixture — is never read here. It is parsed and walked by
//! `doc_forms.rs` (`call_forms` / `fence_call_forms`), and arity is read only by
//! `doc_forms`.

/// Does `text` call `name`: a `name(` whose parens balance?
///
/// Two things are skipped rather than guessed at: a `name(` whose parens never
/// balance (a call form wrapped across a markdown line), and a match preceded by
/// an identifier character, so `min_clearance(` is not harvested out of a
/// hypothetical `xmin_clearance(`. A text scan cannot tell a call from a
/// mention of one inside an HTML comment, so stripping those is the caller's
/// job.
pub(crate) fn calls(text: &str, name: &str) -> bool {
    let needle = format!("{name}(");
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
        for c in text[open..].chars() {
            match c {
                '(' => depth += 1,
                ')' => {
                    // Cannot underflow: this scan starts AT the `(`, so `depth`
                    // is already >= 1 by the time any `)` is reached. Saturating
                    // here would be WRONG, not safer — it would make the
                    // `== 0` test fire on a stray `)` and accept an unbalanced
                    // call form.
                    depth -= 1;
                    if depth == 0 {
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

/// Every DISTINCT identifier CALLED as `name(` in `text`, in document order.
///
/// Complements [`calls`], which answers "is THIS name called". This answers
/// "which names are called AT ALL" — the direction a phantom-signature check
/// needs, because a phantom name is by definition one nobody thought to ask
/// about. `rotate(geo, axis, angle)` and `translate(geo, vector)` (tasks #5347 /
/// #5364) could only have been found by asking this one.
///
/// Each candidate is CONFIRMED through [`calls`] rather than trusted, so the two
/// scanners cannot disagree about what a call is: a `name(` whose parens never
/// balance (a call form wrapped across a markdown line) is skipped here by
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
        if !calls(text, &name) {
            continue;
        }
        out.push(name);
    }
    out
}

// --- Scanner unit tests ------------------------------------------------------
//
// `oracle_xref_smoke.rs`'s prose gates — the call-form coverage class and the
// registry-truth class — sit downstream of these two scanners, so they are
// pinned directly here rather than only through the chunks.
//
// THE FAILURE THESE CLOSE IS SELF-CONCEALING. `called_names` is the extraction
// path for the registry-truth class: a name the scanner stopped seeing is a name
// that class never checks, so a regression that made a scanner over-skip would
// weaken the gate while leaving it green. Only a direct test over a known input
// can see that.

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

/// A `name(` whose parens never balance is skipped, matching [`calls`].
///
/// The two scanners CONFIRM each other by construction — `called_names` runs
/// every candidate back through `calls` — and this pins that the
/// confirmation actually discriminates. A call form wrapped across a markdown
/// line is the live shape this protects against: half a call is not a call, and
/// counting it would let a prose region claim a call form it never wrote out.
///
/// The skip is PER NAME, not per line, which is the behaviour the assertion
/// below fixes in place: in `translate(box(1mm, 1mm, 1mm),` the OUTER
/// `translate(` never closes and is dropped, while the INNER `box(…)` closes on
/// its own and is kept. That is the right call — `box` really is called
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
/// Both halves matter to the caller: the registry-truth class reports per
/// DISTINCT name, so a phantom mentioned twice is one line rather than two, and
/// the lines come in the order a reader meets the names.
#[test]
fn called_names_dedups_and_preserves_document_order() {
    assert_eq!(
        called_names("polygon(0mm, 0mm) ; nurbs(1, 2) ; polygon(1mm, 1mm) ; nurbs(3, 4)"),
        vec!["polygon".to_string(), "nurbs".to_string()]
    );
}
