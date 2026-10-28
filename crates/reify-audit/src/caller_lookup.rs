//! The caller lookup P1 and P5 H2 share: which of a task's changed symbols
//! has a non-test caller, and which could not be asked about because
//! `find_references` failed.

use crate::{ChangedSymbol, EvidenceRef, JCodemunchCallFailed, JCodemunchOps};

/// Whether `symbol` has a caller outside test code — the one definition of
/// "consumed" that P1 and P5 H2 share (design §5 P1: references filtered to
/// non-`*/tests/*`).
///
/// `Err` means the lookup itself failed, so the symbol was NOT examined: it
/// is neither consumed nor an orphan, and [`lookup_callers`] records it
/// skipped.
fn has_non_test_caller(
    jcodemunch: &dyn JCodemunchOps,
    symbol: &ChangedSymbol,
) -> Result<bool, JCodemunchCallFailed> {
    Ok(jcodemunch
        .find_references(symbol)?
        .iter()
        .any(|r| !crate::is_test_path(&r.file)))
}

/// Asks [`has_non_test_caller`] about each of `symbols`, which the detector
/// has already passed through its own per-symbol guards. A symbol with a
/// non-test caller is consumed and dropped; one without is `unconsumed`; one
/// whose lookup failed is skipped, being neither, and the remaining symbols
/// are still asked about.
pub(crate) fn lookup_callers(
    jcodemunch: &dyn JCodemunchOps,
    symbols: impl IntoIterator<Item = ChangedSymbol>,
) -> CallerLookups {
    let mut lookups = CallerLookups {
        unconsumed: Vec::new(),
        skipped: Vec::new(),
    };
    for symbol in symbols {
        match has_non_test_caller(jcodemunch, &symbol) {
            Ok(true) => {}
            Ok(false) => lookups.unconsumed.push(symbol),
            Err(failure) => lookups.skipped.push((symbol, failure)),
        }
    }
    lookups
}

/// What [`lookup_callers`] found for one task's symbols, each list in
/// examination order.
#[derive(Debug)]
pub(crate) struct CallerLookups {
    /// The symbols with no non-test caller: the detector reports each.
    pub(crate) unconsumed: Vec<ChangedSymbol>,
    skipped: Vec<(ChangedSymbol, JCodemunchCallFailed)>,
}

impl CallerLookups {
    /// The symbols whose lookup failed, as the detector's one unexamined
    /// finding for the task cites them, or `None` when every lookup answered.
    pub(crate) fn skipped(&self) -> Option<SkippedSymbols<'_>> {
        let (_, first_failure) = self.skipped.first()?;
        Some(SkippedSymbols {
            count: self.skipped.len(),
            names: self
                .skipped
                .iter()
                .map(|(symbol, _)| format!("`{}`", symbol.name))
                .collect::<Vec<_>>()
                .join(", "),
            first_failure,
            evidence: self
                .skipped
                .iter()
                .map(|(symbol, _)| EvidenceRef::FileLine {
                    path: symbol.file.clone(),
                    line: symbol.line,
                    symbol: Some(symbol.name.clone()),
                })
                .collect(),
        })
    }
}

/// The skipped symbols of a [`CallerLookups`], ready to cite.
#[derive(Debug)]
pub(crate) struct SkippedSymbols<'a> {
    pub(crate) count: usize,
    /// Every skipped symbol's name in backticks, comma-separated, in
    /// examination order.
    pub(crate) names: String,
    /// The first lookup's failure, standing for all of them.
    pub(crate) first_failure: &'a JCodemunchCallFailed,
    /// One [`EvidenceRef::FileLine`] per skipped symbol, citing its
    /// declaration, in examination order.
    pub(crate) evidence: Vec<EvidenceRef>,
}
