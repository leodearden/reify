//! jcodemunch index identity resolution + freshness gate.
//!
//! Implements `docs/prds/…` §4.2 (repo-identity resolution) and §4.3
//! (freshness precondition) for the P1 producer-orphan detector's
//! jcodemunch seam.
//!
//! ## Why this module exists
//!
//! P1 asks jcodemunch "who references this symbol?". If the index it queries
//! is **stale** (indexed at a different commit) or an **empty husk** (an index
//! row exists but carries zero symbols), the answer comes back as an empty
//! reference list — which P1 faithfully reports as an *orphan*. A silently
//! wrong corpus therefore manufactures false High-severity findings, and an
//! absent corpus manufactures a silent all-clear. Both are vacuity, and both
//! are indistinguishable from a healthy run at the CLI surface.
//!
//! This module closes that by (a) deriving the same repo identity jcodemunch
//! itself derives from a filesystem path, and (b) refusing to run when the
//! index at that identity cannot be shown to be fresh and non-empty.

use std::path::Path;

// -----------------------------------------------------------------------
// §4.2 — repo-identity derivation
// -----------------------------------------------------------------------

/// SHA-1 over `bytes`, lowercase hex.
///
/// **This is a NAMING hash, never a security primitive.** SHA-1 is
/// cryptographically broken and must not be used here for anything but
/// reproducing a name. It exists solely to be byte-identical to Python's
/// `hashlib.sha1` as consumed by jcodemunch's `storage/git_root.py`
/// `_local_repo_name`, which derives a local repo id as
/// `f"{folder_path.name}-{sha1(str(folder_path)).hexdigest()[:8]}"`. If we
/// computed a *different* digest we would gate a *different* index than the
/// one the detector actually queries — the precise vacuity this module exists
/// to close — so exact agreement with that one function is the whole
/// requirement.
///
/// Implemented inline (RFC 3174) rather than via a `sha1` crate: no such
/// crate is in `Cargo.lock` or the local registry cache, so adding one needs
/// a network fetch a sandboxed task worktree is not guaranteed to have, plus
/// workspace-wide `Cargo.lock` churn. The workspace's `sha2` is the wrong
/// primitive. Byte-identity is pinned by NIST vectors plus two measured
/// ground-truth repo ids in this module's tests.
pub fn sha1_hex(bytes: &[u8]) -> String {
    // RFC 3174 §6.1. State is five 32-bit words, big-endian throughout.
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];

    // Message + padding: 0x80, then zeros, then the 64-bit big-endian bit
    // length, sized so the total is a multiple of 64 bytes. A 56-byte
    // message therefore needs a whole second block (1 + 8 > 64 - 56).
    let mut padded = Vec::with_capacity(bytes.len() + 72);
    padded.extend_from_slice(bytes);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        // Expand the 16 input words to 80 (RFC 3174 §6.1 step (b)).
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    h.iter().map(|word| format!("{word:08x}")).collect()
}

/// Derive jcodemunch's local repo id for an **already-absolute** path.
///
/// Mirrors jcodemunch's `_local_repo_name`:
/// `local/` + final path component + `-` + first 8 hex chars of
/// `sha1(path_string)`.
///
/// Deliberately PURE — no filesystem access, no canonicalization — so the
/// derivation can be pinned against measured ground truths on a host where
/// neither path exists. Callers holding a possibly-relative or
/// possibly-uncanonical path want [`resolve_repo_id`] instead.
pub fn repo_id_for_abs_path(abs: &Path) -> String {
    let path_str = abs.to_string_lossy();
    let basename = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        // A path with no final component (e.g. `/`) has no basename;
        // jcodemunch's `Path.name` is likewise empty there.
        .unwrap_or_default();
    let digest = sha1_hex(path_str.as_bytes());
    format!("local/{}-{}", basename, &digest[..8])
}

/// Derive the repo id for a project root as supplied on the command line.
///
/// Canonicalizes first so `.` (the default `--project-root`), a trailing
/// slash, a `/./` segment and a symlinked path all resolve to ONE identity —
/// the same normalization `load_tasks_from_fused_memory` already applies to
/// the project root. Three cosmetic spellings of one directory yielding three
/// index identities would silently gate the wrong index.
///
/// Falls back to the path as given when canonicalize fails (e.g. the root
/// does not exist); the caller's downstream freshness check will then refuse
/// on the absent index rather than this function panicking.
pub fn resolve_repo_id(project_root: &Path) -> String {
    let canonical = std::fs::canonicalize(project_root);
    let target = canonical.as_deref().unwrap_or(project_root);
    repo_id_for_abs_path(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // -------------------------------------------------------------------
    // §4.2 — SHA-1 as a *naming* hash
    //
    // These are NIST FIPS 180-1 / RFC 3174 vectors plus length-boundary
    // inputs. They exist to prove byte-identity with Python's
    // `hashlib.sha1`, which is what jcodemunch uses to name a local repo.
    // Every expected digest below was computed live with
    // `python3 -c "import hashlib; print(hashlib.sha1(b'…').hexdigest())"`.
    // -------------------------------------------------------------------

    #[test]
    fn sha1_hex_matches_nist_empty_vector() {
        assert_eq!(
            sha1_hex(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "SHA-1 of the empty string is the canonical NIST vector"
        );
    }

    #[test]
    fn sha1_hex_matches_nist_abc_vector() {
        assert_eq!(
            sha1_hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d",
            "SHA-1 of \"abc\" is the canonical NIST vector"
        );
    }

    #[test]
    fn sha1_hex_matches_nist_two_block_vector() {
        // 56 bytes — the classic NIST multi-block vector. A 56-byte message
        // cannot fit its 0x80 pad byte *and* its 8-byte length in one
        // 64-byte block, so this forces the two-block padding path where
        // hand-rolled SHA-1 implementations classically go wrong.
        let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(msg.len(), 56, "vector length is the point of this test");
        assert_eq!(sha1_hex(msg), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
    }

    #[test]
    fn sha1_hex_handles_padding_length_boundaries() {
        // 55 bytes: the LARGEST message that still pads into a single block
        // (55 + 1 pad + 8 length = 64). One byte more needs a second block.
        assert_eq!(
            sha1_hex(&[b'a'; 55]),
            "c1c8bbdc22796e28c0e15163d20899b65621d65a",
            "55 bytes is the single-block padding boundary"
        );
        // 56 bytes: the first length that spills into a second block.
        assert_eq!(
            sha1_hex(&[b'a'; 56]),
            "c2db330f6083854c99d4b5bfb6e8f29f201be699",
            "56 bytes is the first two-block length"
        );
        // 64 bytes: exactly one full block of message, so padding occupies a
        // whole extra block on its own.
        assert_eq!(
            sha1_hex(&[b'a'; 64]),
            "0098ba824b5c16427bd7a1122a5a442a25ec644d",
            "64 bytes is an exact block multiple"
        );
        // 119 bytes: spans two message blocks with a spilling pad.
        assert_eq!(
            sha1_hex(&[b'a'; 119]),
            "ee971065aaa017e0632a8ca6c77bb3bf8b1dfc56"
        );
    }

    // -------------------------------------------------------------------
    // §4.2 — repo identity derivation
    //
    // `repo_id_for_abs_path` is deliberately PURE (it takes an
    // already-absolute path and touches no filesystem) so these two
    // measured ground truths can be asserted on any host, including one
    // where neither path exists.
    // -------------------------------------------------------------------

    #[test]
    fn repo_id_for_abs_path_matches_measured_reify_ground_truth() {
        assert_eq!(
            repo_id_for_abs_path(Path::new("/home/leo/src/reify")),
            "local/reify-4ae45bbd",
            "measured ground truth: jcodemunch names the reify checkout \
             local/reify-4ae45bbd"
        );
    }

    #[test]
    fn repo_id_for_abs_path_matches_measured_worktree_ground_truth() {
        // Independent cross-check against a second measured identity, so a
        // coincidental single-path match cannot pass this suite.
        assert_eq!(
            repo_id_for_abs_path(Path::new("/home/leo/src/dark-factory/.worktrees/3484")),
            "local/3484-e93a7bf3"
        );
    }

    #[test]
    fn repo_id_for_abs_path_is_pure_and_needs_no_such_path() {
        // A path that cannot exist still yields a well-formed identity:
        // proof the function performs no filesystem access.
        let id = repo_id_for_abs_path(Path::new("/nonexistent-abcxyz/some/proj"));
        assert!(id.starts_with("local/proj-"), "got {id}");
        assert_eq!(id.len(), "local/proj-".len() + 8, "8 hex chars of sha1");
    }

    #[test]
    fn resolve_repo_id_normalizes_cosmetic_path_variants() {
        // The default `--project-root` is `.`, and callers splice paths
        // together in scripts. Three cosmetic spellings of ONE directory must
        // never yield three different index identities — that would silently
        // gate the wrong index.
        let base = env!("CARGO_MANIFEST_DIR");
        let plain = resolve_repo_id(Path::new(base));
        let trailing_slash = resolve_repo_id(Path::new(&format!("{base}/")));
        let dot_segment = resolve_repo_id(Path::new(&format!("{base}/./")));

        assert_eq!(plain, trailing_slash, "trailing slash must not change identity");
        assert_eq!(plain, dot_segment, "a /./ segment must not change identity");
        assert!(
            plain.starts_with("local/"),
            "derived ids are local/-scoped, got {plain}"
        );
    }
}
