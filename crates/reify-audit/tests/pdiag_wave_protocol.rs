//! Wave-protocol contract pin for the PDIAG ratchet (task #6768).
//! Module doc expanded in step-2; this is the first RED test.

#[test]
fn a_wave_that_codes_sites_and_shrinks_its_row_in_the_same_diff_stays_clean() {
    let n = 5usize;
    let k = 2usize;

    // BEFORE: the wave hasn't landed yet — N code-less sites, baseline at N.
    let before = wave_tree(n, 0, Some(n as u32));
    assert_eq!(before, Vec::new(), "BEFORE: N code-less sites at a baseline of N must be clean");

    // AFTER: the wave diff codes K of them and shrinks the row by K in the
    // SAME diff. Assert emptiness of the WHOLE finding list — the absence of
    // the Medium `pdiag-baseline-stale` advisory is half the claim, since
    // that is what distinguishes a same-diff shrink from a deferred one.
    let after = wave_tree(n - k, k, Some((n - k) as u32));
    assert_eq!(
        after,
        Vec::new(),
        "AFTER: coding K sites and shrinking the row in the same diff must stay clean — \
         no High, and no Medium pdiag-baseline-stale advisory either"
    );
}
