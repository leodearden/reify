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
// G-allow: public for the integration-test fixture `tests/common/index_fixture.rs`, which derives the expected repo id with this *verified* primitive rather than re-implementing SHA-1 a second time; in-crate production callers reach it via repo_id_for_abs_path; byte-identity pinned by NIST vectors plus two measured ground-truth repo ids in this module's unit tests.
pub fn sha1_hex(bytes: &[u8]) -> String {
    // RFC 3174 §6.1. State is five 32-bit words, big-endian throughout.
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

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
///
/// Crate-private: [`resolve_repo_id`] is the only production entry point, and
/// this module's unit tests reach the pure form directly. Nothing outside the
/// crate consumes it, so it stays off the public surface.
fn repo_id_for_abs_path(abs: &Path) -> String {
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

// -----------------------------------------------------------------------
// §4.3 — index state, read read-only off disk
// -----------------------------------------------------------------------

/// What the on-disk jcodemunch index claims about itself.
///
/// The three fields are deliberately independent so no probe can mask
/// another: a missing `git_head` row must not suppress a readable symbol
/// count, and neither must be conflated with the file being unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexState {
    /// `meta.git_head` — the commit the corpus was indexed at. `None` means
    /// the index is absent, has no such row, or could not be read.
    pub index_head: Option<String>,
    /// `select count(*) from symbols`. Zero for an absent or husk index.
    pub symbol_count: i64,
    /// `Some(msg)` when the DB file EXISTS but could not be opened or
    /// queried — a corrupt file, a permissions problem, or jcodemunch having
    /// changed its schema. Stays `None` for a plainly-absent index, which is
    /// the distinction that keeps a schema change from being reported as a
    /// routine empty index.
    pub unreadable: Option<String>,
}

impl IndexState {
    /// The state of an index that simply is not there.
    fn absent() -> Self {
        Self {
            index_head: None,
            symbol_count: 0,
            unreadable: None,
        }
    }
}

/// Where jcodemunch stores the index for `repo_id`.
///
/// The filename is the repo id with `/` flattened to `-`, plus `.db` —
/// measured live: `local/1074-352ad3a3` ⇒ `~/.code-index/local-1074-352ad3a3.db`.
///
/// Crate-private: [`read_index_state`] is the only consumer. The integration
/// fixture deliberately re-derives this filename by hand instead of calling it,
/// so the tests hold an independent opinion about the path the operator would
/// predict — calling the function under test to compute its own expected value
/// would let a wrong-but-self-consistent derivation pass.
fn index_db_path(index_dir: &Path, repo_id: &str) -> std::path::PathBuf {
    index_dir.join(format!("{}.db", repo_id.replace('/', "-")))
}

/// Probe the on-disk index for `repo_id` **without modifying it**.
///
/// Opened `SQLITE_OPEN_READ_ONLY` with no `SQLITE_OPEN_CREATE`:
/// `rusqlite::Connection::open` would CREATE a missing file, and against a
/// derived path under the operator's real `~/.code-index` that would litter a
/// phantom zero-symbol index which jcodemunch then registers as an empty
/// repo — manufacturing the exact empty-husk condition this gate exists to
/// detect, so the next run would legitimately refuse against our own
/// artifact. A read-only open errors on a missing file instead, which is the
/// "index absent" arm.
///
/// Never panics: every failure becomes a field on the returned state.
pub fn read_index_state(index_dir: &Path, repo_id: &str) -> IndexState {
    let path = index_db_path(index_dir, repo_id);
    if !path.exists() {
        return IndexState::absent();
    }

    let conn = match rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => conn,
        // The file exists but will not open: corrupt, unreadable, or not a
        // SQLite database. That is a diagnostic, not an empty index.
        Err(e) => {
            return IndexState {
                unreadable: Some(e.to_string()),
                ..IndexState::absent()
            };
        }
    };

    // Two independent probes. Each records its own failure so a missing
    // `meta` row cannot hide a readable `symbols` count, and a dropped
    // `symbols` table cannot hide a readable head.
    let mut unreadable: Option<String> = None;
    let mut note = |e: rusqlite::Error| {
        if unreadable.is_none() {
            unreadable = Some(e.to_string());
        }
    };

    let index_head =
        match conn.query_row("select value from meta where key = 'git_head'", [], |row| {
            row.get::<_, String>(0)
        }) {
            Ok(sha) => Some(sha),
            // No `git_head` row is an expected shape, not a fault.
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                note(e);
                None
            }
        };

    let symbol_count = match conn.query_row("select count(*) from symbols", [], |row| {
        row.get::<_, i64>(0)
    }) {
        Ok(n) => n,
        Err(e) => {
            note(e);
            0
        }
    };

    IndexState {
        index_head,
        symbol_count,
        unreadable,
    }
}

// -----------------------------------------------------------------------
// §4.3 — the freshness decision
// -----------------------------------------------------------------------

/// The index was built at a different commit than the working tree.
///
/// Stable, greppable machine token. Consumers must branch on this rather
/// than on message prose.
pub const E_JC_INDEX_STALE: &str = "E_JC_INDEX_STALE";

/// The index carries no symbols, or does not exist at all.
pub const E_JC_INDEX_EMPTY: &str = "E_JC_INDEX_EMPTY";

/// Why a jcodemunch-backed run was refused before querying any detector.
///
/// Carries §4.3's three quantities so the rendered message is actionable:
/// which commit the corpus was built at, which commit the tree is at now,
/// and how big the corpus was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRefusal {
    code: &'static str,
    /// The repo identity that was probed — the operator needs it to know
    /// *which* index to rebuild.
    pub repo_id: String,
    pub index_head: Option<String>,
    pub live_head: String,
    pub symbol_count: i64,
    /// Propagated from [`IndexState::unreadable`] so a jcodemunch schema
    /// change is diagnosable rather than silently reported as an empty index.
    pub unreadable: Option<String>,
}

impl IndexRefusal {
    /// The machine-readable marker token: [`E_JC_INDEX_STALE`] or
    /// [`E_JC_INDEX_EMPTY`].
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Attach the probed repo identity to a refusal built by
    /// [`evaluate_freshness`], which is identity-agnostic by design.
    pub fn with_repo_id(mut self, repo_id: &str) -> Self {
        self.repo_id = repo_id.to_string();
        self
    }

    /// Attach the reader's schema-drift diagnostic, if any.
    pub fn with_unreadable(mut self, unreadable: Option<String>) -> Self {
        self.unreadable = unreadable;
        self
    }
}

impl std::fmt::Display for IndexRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = if self.code == E_JC_INDEX_STALE {
            "is stale"
        } else {
            "is empty or absent"
        };
        // An absent head gets an explicit marker: printing nothing would read
        // as a truncated message and would not distinguish "no index at all"
        // from "index at head <blank>".
        let head = self.index_head.as_deref().unwrap_or("<absent>");
        write!(
            f,
            "{}: jcodemunch index for {} {} — index_head={} live_head={} symbol_count={}",
            self.code, self.repo_id, what, head, self.live_head, self.symbol_count
        )?;
        if let Some(diag) = &self.unreadable {
            write!(f, " (index unreadable: {diag})")?;
        }
        write!(
            f,
            "; re-index this checkout before querying, or pass --no-jcodemunch \
             to skip the jcodemunch-backed detectors"
        )
    }
}

/// Decide whether `state` is a corpus worth querying, per §4.3.
///
/// Three-arm ladder, in this order — the order is the contract, pinned by
/// tests, not an artifact of statement sequence:
///
/// 1. no `index_head` ⇒ [`E_JC_INDEX_EMPTY`]. An absent index is the limiting
///    case of an empty one; it is classified EMPTY rather than STALE because
///    there is no head to name and the refusal is required to name it.
/// 2. `index_head != live_head` ⇒ [`E_JC_INDEX_STALE`]. Queries answer about
///    a different commit's code, so P1 reports symbols as orphaned that the
///    current tree references.
/// 3. `symbol_count == 0` ⇒ [`E_JC_INDEX_EMPTY`]. The mirror image: every
///    query returns nothing, so every producer looks orphaned.
///
/// Identity-agnostic — the caller attaches the probed `repo_id` via
/// [`IndexRefusal::with_repo_id`] — so the decision stays a pure function of
/// the three §4.3 quantities.
pub fn evaluate_freshness(state: &IndexState, live_head: &str) -> Result<(), IndexRefusal> {
    let refuse = |code: &'static str| IndexRefusal {
        code,
        repo_id: String::new(),
        index_head: state.index_head.clone(),
        live_head: live_head.to_string(),
        symbol_count: state.symbol_count,
        unreadable: None,
    };

    match &state.index_head {
        None => Err(refuse(E_JC_INDEX_EMPTY)),
        Some(head) if head != live_head => Err(refuse(E_JC_INDEX_STALE)),
        Some(_) if state.symbol_count == 0 => Err(refuse(E_JC_INDEX_EMPTY)),
        Some(_) => Ok(()),
    }
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

    // -------------------------------------------------------------------
    // Index-state reader — path derivation and read-only probing
    //
    // The schema below is verbatim from a live jcodemunch index re-probed
    // this session: `CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT)`
    // with a `git_head` row, alongside a `symbols` table.
    // -------------------------------------------------------------------

    /// Write a synthetic index DB for `repo_id` under `dir`.
    ///
    /// `git_head`: `Some(sha)` writes the `meta.git_head` row; `None` omits it
    /// (the row-missing shape). `symbol_rows` seeds that many `symbols` rows —
    /// zero leaves the empty-husk shape a `delete-index` leaves behind.
    fn write_index_db(
        dir: &Path,
        repo_id: &str,
        git_head: Option<&str>,
        symbol_rows: usize,
    ) -> std::path::PathBuf {
        let path = index_db_path(dir, repo_id);
        let conn = rusqlite::Connection::open(&path).expect("open synthetic index db");
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);\
             CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT, path TEXT);",
        )
        .expect("create index schema");
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('index_version', '16')",
            [],
        )
        .expect("insert index_version");
        if let Some(sha) = git_head {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('git_head', ?)",
                rusqlite::params![sha],
            )
            .expect("insert git_head");
        }
        for i in 0..symbol_rows {
            conn.execute(
                "INSERT INTO symbols (name, path) VALUES (?, 'src/lib.rs')",
                rusqlite::params![format!("sym_{i}")],
            )
            .expect("insert symbol");
        }
        path
    }

    #[test]
    fn index_db_path_maps_repo_id_to_flattened_db_filename() {
        let dir = Path::new("/tmp/ix");
        // Measured live: `local/1074-352ad3a3` is stored as
        // `~/.code-index/local-1074-352ad3a3.db`.
        assert_eq!(
            index_db_path(dir, "local/reify-4ae45bbd"),
            dir.join("local-reify-4ae45bbd.db")
        );
        // The `--jcodemunch-repo` override can carry any slash form, so the
        // flattening must apply there too — otherwise the gate would probe a
        // different file than the override selects. Spelled with a neutral
        // `<owner>/<project>` placeholder rather than a real git identity: the
        // flattening behaviour under test is a property of the SHAPE, and the
        // capability manifest greps this tree for the concrete legacy default
        // with `expect: absent`.
        assert_eq!(
            index_db_path(dir, "owner/project"),
            dir.join("owner-project.db")
        );
    }

    #[test]
    fn read_index_state_reads_populated_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_index_db(tmp.path(), "local/proj-deadbeef", Some("abc123"), 3);

        let state = read_index_state(tmp.path(), "local/proj-deadbeef");
        assert_eq!(state.index_head.as_deref(), Some("abc123"));
        assert_eq!(state.symbol_count, 3);
        assert_eq!(state.unreadable, None, "a healthy index is not unreadable");
    }

    #[test]
    fn read_index_state_reads_empty_husk_index() {
        // The shape `delete-index` leaves behind: the index EXISTS and knows
        // its commit, but carries no symbols. Presence proving nothing is the
        // entire reason §4.3 needs a symbol_count conjunct as well as a head
        // comparison.
        let tmp = tempfile::tempdir().expect("tempdir");
        write_index_db(tmp.path(), "local/proj-deadbeef", Some("abc123"), 0);

        let state = read_index_state(tmp.path(), "local/proj-deadbeef");
        assert_eq!(state.index_head.as_deref(), Some("abc123"));
        assert_eq!(state.symbol_count, 0, "empty husk carries zero symbols");
        assert_eq!(state.unreadable, None, "a husk is readable, just empty");
    }

    #[test]
    fn read_index_state_on_absent_db_reports_absence_without_creating_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let expected = index_db_path(tmp.path(), "local/proj-deadbeef");
        assert!(!expected.exists(), "precondition: no index db yet");

        let state = read_index_state(tmp.path(), "local/proj-deadbeef");
        assert_eq!(state.index_head, None);
        assert_eq!(state.symbol_count, 0);
        assert_eq!(
            state.unreadable, None,
            "a plainly-absent index is not a schema-drift diagnostic — that \
             distinction is what keeps a future jcodemunch schema change from \
             masquerading as a routine empty index"
        );

        // Load-bearing: `Connection::open` CREATES a missing file. Against a
        // derived path under the operator's real ~/.code-index that would
        // litter a phantom zero-symbol index, which jcodemunch would then
        // register as an empty repo — manufacturing the exact husk this gate
        // exists to detect. The read-only open must leave no trace.
        assert!(
            !expected.exists(),
            "read_index_state must NOT create the index db it fails to find"
        );
    }

    #[test]
    fn read_index_state_tolerates_missing_git_head_row() {
        // A populated corpus whose provenance row is absent: the symbol count
        // must still be read, so the two probes cannot suppress each other.
        let tmp = tempfile::tempdir().expect("tempdir");
        write_index_db(tmp.path(), "local/proj-deadbeef", None, 7);

        let state = read_index_state(tmp.path(), "local/proj-deadbeef");
        assert_eq!(state.index_head, None, "no git_head row to read");
        assert_eq!(state.symbol_count, 7, "symbol count is read independently");
        assert_eq!(state.unreadable, None);
    }

    #[test]
    fn read_index_state_reports_schema_drift_as_a_distinguishable_diagnostic() {
        // A valid SQLite file carrying NEITHER expected table — what a future
        // jcodemunch schema change looks like from here. It must not panic,
        // and it must NOT be indistinguishable from a routine empty index:
        // the `unreadable` diagnostic is what makes a schema change surface as
        // its own message instead of a silent "index is empty".
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = index_db_path(tmp.path(), "local/proj-deadbeef");
        let conn = rusqlite::Connection::open(&path).expect("open drifted db");
        conn.execute_batch("CREATE TABLE something_else (x INTEGER);")
            .expect("create unrelated table");
        drop(conn);

        let state = read_index_state(tmp.path(), "local/proj-deadbeef");
        assert_eq!(state.index_head, None);
        assert_eq!(state.symbol_count, 0);
        let diag = state
            .unreadable
            .as_deref()
            .expect("schema drift must carry a diagnostic, not read as empty");
        assert!(
            !diag.is_empty(),
            "the diagnostic must actually say something"
        );
    }

    // -------------------------------------------------------------------
    // §4.3 — the freshness decision
    //
    // Assertions bind to CODE IDENTITY (`refusal.code()` vs the public
    // constants), never to message substrings: a diagnostic's machine
    // contract is its code, and a prose-substring assertion would both
    // break on rewording and fail to distinguish the two refusal kinds.
    // Rendered prose is asserted separately, and only for §4.3's three
    // quantities.
    // -------------------------------------------------------------------

    /// An `IndexState` with the three §4.3 quantities set explicitly.
    fn state(index_head: Option<&str>, symbol_count: i64) -> IndexState {
        IndexState {
            index_head: index_head.map(str::to_string),
            symbol_count,
            unreadable: None,
        }
    }

    const LIVE: &str = "1111111111111111111111111111111111111111";
    const OTHER: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn evaluate_freshness_admits_a_fresh_populated_index() {
        assert!(
            evaluate_freshness(&state(Some(LIVE), 42), LIVE).is_ok(),
            "an index at the live commit with symbols in it must be admitted"
        );
    }

    #[test]
    fn evaluate_freshness_refuses_a_stale_index() {
        let refusal = evaluate_freshness(&state(Some(OTHER), 42), LIVE)
            .expect_err("an index at a different commit must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_STALE);
    }

    #[test]
    fn evaluate_freshness_refuses_an_empty_husk_index() {
        // Fresh head, zero symbols: the mirror-image failure. Every query
        // returns nothing, so P1 reports every producer as an orphan while
        // the head comparison alone would have said "all good".
        let refusal = evaluate_freshness(&state(Some(LIVE), 0), LIVE)
            .expect_err("an index with zero symbols must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_EMPTY);
    }

    #[test]
    fn evaluate_freshness_refuses_an_absent_index_as_empty() {
        // Precedence arm 1. Classified EMPTY, not STALE: there is no
        // index_head to name, and §4.3 requires the refusal to name it —
        // calling a nonexistent head "stale" would be nonsense.
        let refusal =
            evaluate_freshness(&state(None, 0), LIVE).expect_err("an absent index must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_EMPTY);
    }

    #[test]
    fn evaluate_freshness_resolves_both_degenerate_to_stale() {
        // Heads differ AND the corpus is empty. Pinning STALE here fixes
        // §4.3's left-to-right conjunct order, so the outcome is deterministic
        // rather than an incidental consequence of statement order.
        let refusal = evaluate_freshness(&state(Some(OTHER), 0), LIVE)
            .expect_err("a stale AND empty index must be refused");
        assert_eq!(
            refusal.code(),
            E_JC_INDEX_STALE,
            "staleness is evaluated before emptiness"
        );
    }

    #[test]
    fn refusal_message_names_all_three_freshness_quantities() {
        // §4.3's prose requirement coexists with the code rather than being
        // replaced by it: an operator reading stderr needs to see WHICH heads
        // disagreed and how big the corpus was, or the refusal is unactionable.
        let stale = evaluate_freshness(&state(Some(OTHER), 42), LIVE).unwrap_err();
        let msg = stale.to_string();
        assert!(
            msg.contains(E_JC_INDEX_STALE),
            "code token missing from {msg}"
        );
        assert!(msg.contains(OTHER), "index_head missing from {msg}");
        assert!(msg.contains(LIVE), "live_head missing from {msg}");
        assert!(msg.contains("42"), "symbol_count missing from {msg}");

        let empty = evaluate_freshness(&state(Some(LIVE), 0), LIVE).unwrap_err();
        let msg = empty.to_string();
        assert!(
            msg.contains(E_JC_INDEX_EMPTY),
            "code token missing from {msg}"
        );
        assert!(msg.contains(LIVE), "heads missing from {msg}");
        assert!(msg.contains('0'), "symbol_count missing from {msg}");
    }

    #[test]
    fn absent_index_refusal_marks_the_head_explicitly_absent() {
        // With no index_head there is nothing to print, but printing NOTHING
        // would read as a truncated message. An explicit absent-marker keeps
        // "no index at all" distinguishable from "index at head <blank>".
        let refusal = evaluate_freshness(&state(None, 0), LIVE).unwrap_err();
        let msg = refusal.to_string();
        assert!(
            msg.contains(E_JC_INDEX_EMPTY),
            "code token missing from {msg}"
        );
        assert!(msg.contains(LIVE), "live_head missing from {msg}");
        assert!(
            msg.contains("<absent>"),
            "an absent index_head needs an explicit marker, got {msg}"
        );
    }

    #[test]
    fn the_two_marker_codes_are_distinct_non_empty_tokens() {
        // A refactor that collapsed these to one token, or emptied either,
        // would silently destroy every machine consumer's ability to tell a
        // stale corpus from an absent one while all the code() assertions
        // above still passed.
        assert_ne!(E_JC_INDEX_STALE, E_JC_INDEX_EMPTY);
        assert!(!E_JC_INDEX_STALE.is_empty());
        assert!(!E_JC_INDEX_EMPTY.is_empty());
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

        assert_eq!(
            plain, trailing_slash,
            "trailing slash must not change identity"
        );
        assert_eq!(plain, dot_segment, "a /./ segment must not change identity");
        assert!(
            plain.starts_with("local/"),
            "derived ids are local/-scoped, got {plain}"
        );
    }
}
