//! Wire mirror of a solve's diagnostics and structured detail: the bytes of
//! the length-framed prefix block in a
//! [`WithDiagnostics`](super::WithDiagnostics) cache entry.
//!
//! The live types are deliberately serde-free, so this cache owns its wire
//! format through private mirror types. [`encode_block`] and [`decode_block`]
//! are the whole interface; the length frame and its bound belong to the
//! envelope.

use std::io;

use reify_compute_contract::StructuredComputeDetail;
use reify_solver_elastic::{DofDirection, ElementId, FeaDiagnosticDetail};
use serde::{Deserialize, Serialize};

/// On-disk wire mirror of [`reify_core::Diagnostic`].
///
/// A mirror rather than a serde derive on `Diagnostic` itself: `Diagnostic` is
/// `#[non_exhaustive]` and carries no serde impls, and this cache must own its
/// wire format independently of that type's evolution. reify-shell-extract's
/// `DiagnosticOnDisk` is not reused because it drops `code`, which LSP,
/// `--json` output and this cache's acceptance tests key off.
///
/// `severity`, `message`, `code` and `candidates` are carried; `labels` is
/// not. A field `Diagnostic` gains upstream takes its builder default here
/// until this mirror is extended.
///
/// # Why `labels` are not carried
///
/// A label anchors its message to a [`reify_core::SourceSpan`]: absolute byte
/// offsets into one source text. This cache's key does not identify that text
/// — `Value::content_hash` excludes the `@@source_span` overlay by design, and
/// `compute_cache_key` sees no spans — so two source layouts with identical
/// FEA inputs share one entry. A replayed span would anchor into the wrong
/// text, and `byte_offset_to_line_col` `debug_assert!`-panics when that text
/// is the shorter one. A warm serve therefore replays an UNANCHORED
/// diagnostic: less precise than the cold one, never mis-pointing. The label's
/// text survives in practice, because `fea_diagnostic_to_core` labels with the
/// diagnostic's own `message`.
///
/// # Why `code` is persisted as a NAME
///
/// bincode encodes an enum as its positional variant index. `DiagnosticCode`
/// is grouped by category, so new codes are inserted mid-enum.
/// [`ENTRY_FORMAT_VERSION`](super::ENTRY_FORMAT_VERSION) does not move when
/// that happens; [`ENGINE_VERSION_HASH`](super::ENGINE_VERSION_HASH) now does,
/// because reify-core is hashed, but a name keeps the wire format
/// independent of variant order instead of relying on that. An index would
/// silently re-read every entry as its neighbouring code; a name makes an
/// insertion a non-event and degrades a rename or removal to `None`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct PersistedDiagnostic {
    /// Encoded by [`severity_to_u8`]; an unknown byte is rejected loudly by
    /// [`severity_from_u8`] rather than deserialised into a default.
    severity: u8,
    message: String,
    /// The stable serde variant name, never the positional index.
    code: Option<String>,
    candidates: Vec<String>,
}

/// Encode a [`reify_core::Severity`] as its on-disk `u8` discriminant.
fn severity_to_u8(s: reify_core::Severity) -> u8 {
    match s {
        reify_core::Severity::Info => 0,
        reify_core::Severity::Warning => 1,
        reify_core::Severity::Error => 2,
    }
}

/// Decode an on-disk severity discriminant, rejecting unknown values with
/// `InvalidData` so a corrupt or tampered entry surfaces as a cache miss
/// rather than as a silently-wrong severity.
fn severity_from_u8(b: u8) -> io::Result<reify_core::Severity> {
    match b {
        0 => Ok(reify_core::Severity::Info),
        1 => Ok(reify_core::Severity::Warning),
        2 => Ok(reify_core::Severity::Error),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "PersistedDiagnostic unknown Severity discriminant {other} \
                 (corrupted or tampered cache entry?)"
            ),
        )),
    }
}

/// Encode a [`reify_core::DiagnosticCode`] as its stable on-disk name.
///
/// The name is `DiagnosticCode`'s own serde identifier, so it stays in
/// lock-step with the PascalCase wire format reify-core already test-pins for
/// LSP and `--json` consumers, with no second table to rot.
///
/// A code that does not serialise to a plain name (a data-carrying variant,
/// say) is persisted uncoded, the same bounded degradation
/// [`code_from_wire_name`] applies on read — and, like it, never silently.
fn code_to_wire_name(c: reify_core::DiagnosticCode) -> Option<String> {
    match serde_json::to_value(c) {
        Ok(serde_json::Value::String(name)) => Some(name),
        other => {
            tracing::warn!(
                code = ?c,
                encoded = ?other,
                "DiagnosticCode does not serialise to a plain name; \
                 persisting the diagnostic without a code"
            );
            None
        }
    }
}

/// Decode an on-disk code name, degrading to `None` when this build does not
/// know it.
///
/// Unlike [`severity_from_u8`], an unrecognised name is NOT `InvalidData`:
/// dropping one code is a bounded, safe degradation, whereas rejecting the
/// entry would throw away a still-valid cached solve. The `tracing::warn!`
/// keeps that degradation observable.
fn code_from_wire_name(name: &str) -> Option<reify_core::DiagnosticCode> {
    match serde_json::from_value(serde_json::Value::String(name.to_owned())) {
        Ok(code) => Some(code),
        Err(_) => {
            tracing::warn!(
                code_name = name,
                "persistent cache entry carries an unrecognised DiagnosticCode name; \
                 replaying the diagnostic without a code"
            );
            None
        }
    }
}

/// Project a live [`reify_core::Diagnostic`] onto its wire mirror.
fn diagnostic_to_persisted(d: &reify_core::Diagnostic) -> PersistedDiagnostic {
    PersistedDiagnostic {
        severity: severity_to_u8(d.severity),
        message: d.message.clone(),
        code: d.code.and_then(code_to_wire_name),
        candidates: d.candidates.clone(),
    }
}

/// Rehydrate a wire mirror into a live [`reify_core::Diagnostic`].
///
/// Built through the public builders rather than a struct literal —
/// `Diagnostic` is `#[non_exhaustive]`, so a literal would not compile from
/// outside reify-core and would silently need updating on every new field.
fn diagnostic_from_persisted(p: &PersistedDiagnostic) -> io::Result<reify_core::Diagnostic> {
    let mut out = match severity_from_u8(p.severity)? {
        reify_core::Severity::Info => reify_core::Diagnostic::info(p.message.clone()),
        reify_core::Severity::Warning => reify_core::Diagnostic::warning(p.message.clone()),
        reify_core::Severity::Error => reify_core::Diagnostic::error(p.message.clone()),
    };
    if let Some(code) = p.code.as_deref().and_then(code_from_wire_name) {
        out = out.with_code(code);
    }
    if !p.candidates.is_empty() {
        out = out.with_candidates(p.candidates.clone());
    }
    Ok(out)
}

/// On-disk wire mirror of [`StructuredComputeDetail`].
///
/// A mirror for the same reason as [`PersistedDiagnostic`]: the live type is
/// deliberately serde-free, and this cache owns its wire format. Unlike
/// `DiagnosticCode`, this enum is owned HERE, so bincode's positional variant
/// tag is a local wire-format fact: declaration order IS the on-disk tag, as
/// [`CacheEntryHeader`](super::CacheEntryHeader)'s field order is. Append
/// only — a reorder or an insertion needs an
/// [`ENTRY_FORMAT_VERSION`](super::ENTRY_FORMAT_VERSION) bump. Pinned by the
/// parent's `with_diagnostics_prefix_encoding_matches_pinned_bytes`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum PersistedStructuredDetail {
    Fea(PersistedFeaDetail),
}

/// On-disk wire mirror of [`FeaDiagnosticDetail`]. Declaration order is the
/// wire tag, exactly as for [`PersistedStructuredDetail`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum PersistedFeaDetail {
    /// Each mode encoded by [`dof_direction_to_u8`].
    Unconstrained {
        rigid_body_modes: Vec<u8>,
    },
    /// Each [`ElementId`] widened to a platform-independent `u64`.
    ProblemElements {
        element_ids: Vec<u64>,
    },
    UnresolvedSelector {
        selector_path: String,
    },
}

/// Encode a [`DofDirection`] as its on-disk `u8` code: 0..=5 in
/// [`DofDirection::all_rigid_body_modes`] order.
fn dof_direction_to_u8(d: DofDirection) -> u8 {
    match d {
        DofDirection::TranslationX => 0,
        DofDirection::TranslationY => 1,
        DofDirection::TranslationZ => 2,
        DofDirection::RotationX => 3,
        DofDirection::RotationY => 4,
        DofDirection::RotationZ => 5,
    }
}

/// Decode an on-disk DOF code, rejecting unknown values with `InvalidData` so
/// a corrupt or tampered entry surfaces as a cache miss rather than as a
/// silently-wrong direction.
fn dof_direction_from_u8(b: u8) -> io::Result<DofDirection> {
    match b {
        0 => Ok(DofDirection::TranslationX),
        1 => Ok(DofDirection::TranslationY),
        2 => Ok(DofDirection::TranslationZ),
        3 => Ok(DofDirection::RotationX),
        4 => Ok(DofDirection::RotationY),
        5 => Ok(DofDirection::RotationZ),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "PersistedFeaDetail unknown DofDirection code {other} \
                 (corrupted or tampered cache entry?)"
            ),
        )),
    }
}

/// Decode an on-disk element id, rejecting one this platform's `usize` cannot
/// hold rather than truncating it into a different element.
fn element_id_from_u64(id: u64) -> io::Result<ElementId> {
    usize::try_from(id).map(ElementId).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "PersistedFeaDetail element id {id} does not fit in usize \
                 (corrupted or tampered cache entry?)"
            ),
        )
    })
}

/// Project a live overlay onto its wire mirror.
///
/// Both this and [`fea_detail_to_persisted`] match exhaustively with no
/// wildcard, so a new upstream variant is a compile error here rather than a
/// silently dropped overlay: a structured overlay has no meaningful partial
/// form to degrade to.
fn structured_detail_to_persisted(d: &StructuredComputeDetail) -> PersistedStructuredDetail {
    match d {
        StructuredComputeDetail::Fea(fea) => {
            PersistedStructuredDetail::Fea(fea_detail_to_persisted(fea))
        }
    }
}

fn fea_detail_to_persisted(d: &FeaDiagnosticDetail) -> PersistedFeaDetail {
    match d {
        FeaDiagnosticDetail::Unconstrained { rigid_body_modes } => {
            PersistedFeaDetail::Unconstrained {
                rigid_body_modes: rigid_body_modes
                    .iter()
                    .copied()
                    .map(dof_direction_to_u8)
                    .collect(),
            }
        }
        FeaDiagnosticDetail::ProblemElements { ids } => PersistedFeaDetail::ProblemElements {
            element_ids: ids.iter().map(|id| id.0 as u64).collect(),
        },
        FeaDiagnosticDetail::UnresolvedSelector { selector_path } => {
            PersistedFeaDetail::UnresolvedSelector {
                selector_path: selector_path.clone(),
            }
        }
    }
}

/// Rehydrate a wire mirror into a live overlay.
fn structured_detail_from_persisted(
    p: &PersistedStructuredDetail,
) -> io::Result<StructuredComputeDetail> {
    match p {
        PersistedStructuredDetail::Fea(fea) => {
            fea_detail_from_persisted(fea).map(StructuredComputeDetail::Fea)
        }
    }
}

fn fea_detail_from_persisted(p: &PersistedFeaDetail) -> io::Result<FeaDiagnosticDetail> {
    Ok(match p {
        PersistedFeaDetail::Unconstrained { rigid_body_modes } => {
            FeaDiagnosticDetail::Unconstrained {
                rigid_body_modes: rigid_body_modes
                    .iter()
                    .copied()
                    .map(dof_direction_from_u8)
                    .collect::<io::Result<_>>()?,
            }
        }
        PersistedFeaDetail::ProblemElements { element_ids } => {
            FeaDiagnosticDetail::ProblemElements {
                ids: element_ids
                    .iter()
                    .copied()
                    .map(element_id_from_u64)
                    .collect::<io::Result<_>>()?,
            }
        }
        PersistedFeaDetail::UnresolvedSelector { selector_path } => {
            FeaDiagnosticDetail::UnresolvedSelector {
                selector_path: selector_path.clone(),
            }
        }
    })
}

/// The length-framed prefix block of a
/// [`WithDiagnostics`](super::WithDiagnostics) entry.
///
/// Field order IS wire order. `diagnostics` stays first, so its bytes are a
/// byte-identical prefix of the block, exactly as the whole block was in v4.
#[derive(Serialize, Deserialize)]
struct PersistedDiagnosticsBlock {
    diagnostics: Vec<PersistedDiagnostic>,
    structured_detail: Vec<PersistedStructuredDetail>,
}

/// Encode a solve's metadata as a [`PersistedDiagnosticsBlock`].
pub(super) fn encode_block(
    diagnostics: &[reify_core::Diagnostic],
    structured_detail: &[StructuredComputeDetail],
) -> Vec<u8> {
    let block = PersistedDiagnosticsBlock {
        diagnostics: diagnostics.iter().map(diagnostic_to_persisted).collect(),
        structured_detail: structured_detail
            .iter()
            .map(structured_detail_to_persisted)
            .collect(),
    };
    bincode::serialize(&block).expect(
        "PersistedDiagnosticsBlock holds only plain owned records (u8s, \
         u64s, Strings and Vecs of them); bincode::serialize into a Vec \
         cannot fail.",
    )
}

/// Decode a block written by [`encode_block`]. A malformed block, or a record
/// this build cannot represent, is an `Err`, never a silently wrong record.
pub(super) fn decode_block(
    bytes: &[u8],
) -> io::Result<(Vec<reify_core::Diagnostic>, Vec<StructuredComputeDetail>)> {
    let block: PersistedDiagnosticsBlock = bincode::deserialize(bytes).map_err(io::Error::other)?;
    let diagnostics = block
        .diagnostics
        .iter()
        .map(diagnostic_from_persisted)
        .collect::<io::Result<Vec<_>>>()?;
    let structured_detail = block
        .structured_detail
        .iter()
        .map(structured_detail_from_persisted)
        .collect::<io::Result<Vec<_>>>()?;
    Ok((diagnostics, structured_detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_diagnostic_round_trips_severity_message_and_code() {
        let original = reify_core::Diagnostic::warning("shell candidate too thick")
            .with_code(reify_core::DiagnosticCode::ShellTooThick);
        let restored = diagnostic_from_persisted(&diagnostic_to_persisted(&original))
            .expect("a well-formed mirror must decode");

        assert_eq!(
            restored.severity,
            reify_core::Severity::Warning,
            "severity must round-trip"
        );
        assert_eq!(
            restored.message, original.message,
            "message must round-trip verbatim"
        );
        assert_eq!(
            restored.code,
            Some(reify_core::DiagnosticCode::ShellTooThick),
            "code must round-trip — this is the field the shell-extract mirror \
             drops and the one this task exists to carry"
        );
    }

    #[test]
    fn persisted_diagnostic_round_trips_absent_code_as_none() {
        let original = reify_core::Diagnostic::info("adaptive refinement converged");
        let restored = diagnostic_from_persisted(&diagnostic_to_persisted(&original))
            .expect("a well-formed mirror must decode");

        assert_eq!(restored.severity, reify_core::Severity::Info);
        assert_eq!(restored.message, original.message);
        assert_eq!(
            restored.code, None,
            "an uncoded diagnostic must round-trip as None, not as a defaulted code"
        );
    }

    #[test]
    fn persisted_diagnostic_rejects_out_of_range_severity_discriminant() {
        // A corrupted or tampered entry must be rejected loudly rather than
        // silently defaulting to Info — the same posture as
        // `severity_from_u8` in reify-shell-extract.
        let corrupt = PersistedDiagnostic {
            severity: 7,
            message: "corrupt".to_string(),
            code: None,
            candidates: Vec::new(),
        };
        let err = diagnostic_from_persisted(&corrupt)
            .expect_err("an unknown severity discriminant must not decode");
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "expected InvalidData, got {err:?}"
        );
        assert!(
            err.to_string().contains('7'),
            "the rejection must name the offending discriminant, got: {err}"
        );
    }

    #[test]
    fn persisted_diagnostic_code_is_encoded_by_name_not_variant_index() {
        let block = encode_block(
            &[reify_core::Diagnostic::warning("m")
                .with_code(reify_core::DiagnosticCode::ShellTooThick)],
            &[],
        );
        assert!(
            block.windows(13).any(|w| w == b"ShellTooThick"),
            "the encoded block must carry the code's stable NAME, not a \
             positional variant index; bytes: {block:?}"
        );
    }

    #[test]
    fn unrecognised_code_name_decodes_to_none() {
        let from_the_future = PersistedDiagnostic {
            severity: 1,
            message: "written by a newer engine".to_string(),
            code: Some("NoSuchCodeFromTheFuture".to_string()),
            candidates: Vec::new(),
        };
        let restored = diagnostic_from_persisted(&from_the_future)
            .expect("an unknown code name must degrade, never fail the decode");

        assert_eq!(
            restored.code, None,
            "an unknown code name must decode to None, never to a neighbouring variant"
        );
        assert_eq!(
            restored.severity,
            reify_core::Severity::Warning,
            "severity must survive the degradation"
        );
        assert_eq!(
            restored.message, "written by a newer engine",
            "message must survive the degradation"
        );
    }

    #[test]
    fn known_code_names_round_trip() {
        // Both a shell-selection code and an FEA code, from opposite ends of the
        // category-grouped enum, plus the absent case.
        for expected in [
            Some(reify_core::DiagnosticCode::ShellTooThick),
            Some(reify_core::DiagnosticCode::FeaUnderConstrained),
            None,
        ] {
            let mut d = reify_core::Diagnostic::warning("m");
            if let Some(c) = expected {
                d = d.with_code(c);
            }
            let restored = diagnostic_from_persisted(&diagnostic_to_persisted(&d))
                .expect("a well-formed mirror must decode");
            assert_eq!(
                restored.code, expected,
                "code must survive the name-based round trip"
            );
        }
    }
}
