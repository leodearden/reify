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

use std::path::{Path, PathBuf};

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
/// Normalizes the path exactly the way the INDEXER does before hashing it,
/// because an identity that disagrees with the one
/// `scripts/jcodemunch-index-reify.sh` wrote is worse than useless: it names
/// an index that does not exist, so the refusal sends the operator to
/// re-index a phantom. That script does `~`-expansion followed by
/// `readlink -f` (absolutize + resolve symlinks, **non-strict** on a missing
/// leaf), so [`normalize_project_root`] reproduces all three steps:
///
/// 1. a leading `~` / `~/…` expands against `$HOME`;
/// 2. a relative path is joined onto the current directory;
/// 3. symlinks are resolved, falling back to the deepest existing ancestor
///    when the leaf does not exist.
///
/// Step 3 subsumes the cosmetic normalization this function has always done —
/// `.` (the default `--project-root`), a trailing slash, a `/./` segment and
/// a symlinked path all still resolve to ONE identity, the same normalization
/// `load_tasks_from_fused_memory` applies to the project root. Steps 1 and 2
/// are what a bare `canonicalize` could not do: `canonicalize` FAILS outright
/// on a path that does not exist, and the old fallback then hashed the string
/// as given, so `--project-root ~/src/reify` (arriving quoted, never expanded
/// by a shell) or a relative root hashed a spelling the indexer could never
/// produce.
///
/// Never panics and never requires the leaf to exist: a root that is not
/// there yet still yields the identity the indexer *would* use, and the
/// caller's downstream freshness check refuses on the absent index.
pub fn resolve_repo_id(project_root: &Path) -> String {
    repo_id_for_abs_path(&normalize_project_root(project_root))
}

/// `~`-expand, absolutize, then resolve symlinks non-strictly — the
/// `readlink -f` semantics `scripts/jcodemunch-index-reify.sh` applies before
/// deriving the same identity. See [`resolve_repo_id`] for why each step is
/// load-bearing.
fn normalize_project_root(project_root: &Path) -> PathBuf {
    let absolute = absolutize(&expand_tilde(project_root));
    // Whole path exists: `canonicalize` IS `readlink -f`.
    if let Ok(canonical) = std::fs::canonicalize(&absolute) {
        return canonical;
    }
    // Leaf missing: `readlink -f` still resolves the existing prefix and
    // re-joins the rest, so a symlinked parent yields the same identity
    // whether or not the leaf has been created yet.
    if let (Some(parent), Some(leaf)) = (absolute.parent(), absolute.file_name()) {
        if let Ok(canonical_parent) = std::fs::canonicalize(parent) {
            return canonical_parent.join(leaf);
        }
    }
    absolute
}

/// Expand a leading `~` against `$HOME`.
///
/// Only a bare `~` or `~/…` is treated as a home reference — `~user/…` is a
/// shell form the indexer script does not expand either, so guessing at it
/// here would *introduce* a divergence rather than close one. An unset or
/// empty `$HOME` leaves the path untouched for the same reason.
fn expand_tilde(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let Some(rest) = raw.strip_prefix('~') else {
        return path.to_path_buf();
    };
    if !(rest.is_empty() || rest.starts_with('/')) {
        return path.to_path_buf();
    }
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => {
            let trimmed = rest.trim_start_matches('/');
            if trimmed.is_empty() {
                PathBuf::from(home)
            } else {
                PathBuf::from(home).join(trimmed)
            }
        }
        _ => path.to_path_buf(),
    }
}

/// Join a relative path onto the current directory. An already-absolute path
/// passes through; an unreadable cwd leaves it as given, which is the only
/// honest answer available.
fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
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
    /// Whether `symbols` holds at least one row.
    ///
    /// Probed with `select exists(select 1 from symbols)`, which stops at the
    /// first row, rather than `count(*)`, which walks the whole table — on
    /// the real reify index that is 10^5–10^6 rows scanned on EVERY
    /// jcodemunch-backed run (plus page-cache pressure on a DB a live watcher
    /// is writing) for a number the §4.3 ladder only ever compares to zero.
    /// The exact count is wanted solely to *render* a refusal, so it is
    /// fetched there and only there — see [`count_symbols`].
    pub symbols_present: bool,
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
            symbols_present: false,
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

    let conn = match open_read_only(&path) {
        Ok(conn) => conn,
        // The file exists but will not open: corrupt, unreadable, or not a
        // SQLite database. That is a diagnostic, not an empty index.
        Err(e) => {
            return IndexState {
                unreadable: Some(e),
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

    // Emptiness, not the count: `exists(select 1 …)` short-circuits on the
    // first row, so this is O(1) where `count(*)` is a full table walk. See
    // `IndexState::symbols_present`.
    let symbols_present = match conn.query_row("select exists(select 1 from symbols)", [], |row| {
        row.get::<_, i64>(0)
    }) {
        Ok(n) => n != 0,
        Err(e) => {
            note(e);
            false
        }
    };

    IndexState {
        index_head,
        symbols_present,
        unreadable,
    }
}

/// Open `path` read-only **and prove it is actually readable**, with a
/// WAL-safe fallback.
///
/// The first attempt is a plain `SQLITE_OPEN_READ_ONLY` open with no
/// `SQLITE_OPEN_CREATE` — see [`read_index_state`] for why CREATE is
/// forbidden.
///
/// That attempt is then VERIFIED with a trivial schema read, because a
/// successful open is not evidence of a readable database. Measured on the
/// bundled SQLite 3.45: for a database in WAL journal mode carrying an
/// uncheckpointed `-wal`, with no `-shm` beside it and no write access to the
/// containing directory, `open_with_flags` returns `Ok` and the FIRST QUERY
/// fails with `SQLITE_CANTOPEN` / "unable to open database file" — SQLite
/// defers materialising the shared-memory segment until it reads. jcodemunch's
/// index is maintained by a long-running watcher, so this is invisible while
/// the watcher is up and its `-shm` exists, and appears against a corpus that
/// is perfectly intact. Without the probe the failure would surface from
/// whichever of the two state probes ran first, i.e. as an arbitrary one of
/// them rather than as the open fault it is.
///
/// The fallback re-opens the same file through a `file:…?immutable=1` URI,
/// which tells SQLite to read the main database directly and skip the WAL
/// machinery entirely. `immutable=1` is a *promise* the file is not changing,
/// so it is deliberately a fallback and never the first attempt: whenever a
/// writer could be attached, the plain open+probe succeeds and this path is
/// not taken. Its cost is that WAL frames are invisible, so the answer comes
/// from the last checkpointed image — which for §4.3's two questions ("is this
/// corpus at the live head" and "does it hold any symbols") errs toward
/// staleness/emptiness, i.e. fails closed into a refusal rather than into a
/// false all-clear.
fn open_read_only(path: &Path) -> Result<rusqlite::Connection, String> {
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    let first_err = match rusqlite::Connection::open_with_flags(path, flags) {
        Ok(conn) => match probe_readable(&conn) {
            Ok(()) => return Ok(conn),
            Err(e) => e,
        },
        Err(e) => e.to_string(),
    };
    let uri = format!("file:{}?immutable=1", uri_escape(path));
    let fallback = rusqlite::Connection::open_with_flags(
        Path::new(&uri),
        flags | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    );
    match fallback {
        Ok(conn) if probe_readable(&conn).is_ok() => Ok(conn),
        // Report the FIRST error either way. The immutable retry's error would
        // blame the URI form for a fault that is really about the file, which
        // is the opposite of the diagnosis an operator needs.
        _ => Err(first_err),
    }
}

/// The cheapest read that forces SQLite to acquire a read lock — and
/// therefore to materialise a WAL `-shm` segment if one is needed. Reads the
/// schema, which every SQLite database has and which costs one page.
fn probe_readable(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.query_row("select count(*) from sqlite_schema", [], |row| {
        row.get::<_, i64>(0)
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Percent-escape the three characters SQLite's URI parser treats specially,
/// so an index path containing `?`, `#` or `%` survives the `file:` form.
fn uri_escape(path: &Path) -> String {
    let mut out = String::new();
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            _ => out.push(c),
        }
    }
    out
}

/// Exact `select count(*) from symbols` for the index at `repo_id`.
///
/// The O(n) table walk [`read_index_state`] deliberately skips. Call it on
/// the REFUSAL path only, where the message is about to name the count and
/// the run is ending anyway — never as a startup precondition.
///
/// Returns 0 for an absent, unopenable or schema-drifted index, which is the
/// same value §4.3 already reasons about for those shapes.
pub fn count_symbols(index_dir: &Path, repo_id: &str) -> i64 {
    let path = index_db_path(index_dir, repo_id);
    if !path.exists() {
        return 0;
    }
    let Ok(conn) = open_read_only(&path) else {
        return 0;
    };
    conn.query_row("select count(*) from symbols", [], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap_or(0)
}

// -----------------------------------------------------------------------
// Shared synthetic-index writer (test support)
// -----------------------------------------------------------------------

/// Write a synthetic jcodemunch index DB for `repo_id` under `dir`, returning
/// the path written.
///
/// `git_head`: `Some(sha)` writes the `meta.git_head` row; `None` omits it
/// (the row-missing shape). `symbol_rows` seeds that many `symbols` rows —
/// zero leaves the empty-husk shape a `delete-index` leaves behind.
///
/// The schema is verbatim from a live jcodemunch index. That is exactly why
/// this lives here as the ONE writer rather than being copied into
/// `tests/common/index_fixture.rs` as well: it encodes a *captured upstream
/// schema*, it is not the function under test, and a jcodemunch schema change
/// mirrored into only one of two copies would leave the unit suite and the
/// integration suite silently testing different corpora. (`index_db_path` is
/// the deliberate exception — the integration fixture re-derives that by hand
/// so the tests hold an independent opinion about the filename.)
///
/// Gated behind `test-support` so it never reaches a production build; the
/// crate self-pulls that feature in its own `[dev-dependencies]`, which is
/// how `tests/common/index_fixture.rs` reaches it.
#[cfg(any(test, feature = "test-support"))]
// G-allow: the ONE synthetic-index writer, shared by this module's unit tests and the integration fixture `tests/common/index_fixture.rs`; public only under `test-support` (which the crate self-pulls in its own [dev-dependencies]) so integration tests can reach it, never compiled into a production build.
pub fn write_index_db(
    dir: &Path,
    repo_id: &str,
    git_head: Option<&str>,
    symbol_rows: usize,
) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create index dir");
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

/// The index file EXISTS but could not be read — a corrupt file, a
/// permissions problem, a WAL database no fallback could open, or jcodemunch
/// having changed its schema.
///
/// Its own code because its REMEDY differs: "re-index this checkout" is the
/// wrong instruction for an intact corpus behind an unreadable file, and
/// sending an operator to rebuild an index that is not the problem is worse
/// than saying nothing. Keeping it out of [`E_JC_INDEX_EMPTY`] is the same
/// distinction [`IndexState::unreadable`] draws at the reader layer, carried
/// through to the decision layer instead of being collapsed there.
pub const E_JC_INDEX_UNREADABLE: &str = "E_JC_INDEX_UNREADABLE";

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

    /// Attach the exact symbol count, fetched lazily on the refusal path via
    /// [`count_symbols`].
    ///
    /// [`evaluate_freshness`] leaves the field at 0 because the reader
    /// answers emptiness with an O(1) `exists` probe and never learns a
    /// number; 0 is the truthful value for every arm that is *about*
    /// emptiness, and the STALE arm is the only one where an exact count
    /// tells the operator anything.
    pub fn with_symbol_count(mut self, symbol_count: i64) -> Self {
        self.symbol_count = symbol_count;
        self
    }
}

impl std::fmt::Display for IndexRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = if self.code == E_JC_INDEX_STALE {
            "is stale"
        } else if self.code == E_JC_INDEX_UNREADABLE {
            "could not be read"
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
        // The remedy is per-code, not boilerplate. "re-index this checkout"
        // is actively misleading for an intact corpus behind an unreadable
        // file — it points at the wrong artifact and costs a full re-index to
        // learn nothing.
        if self.code == E_JC_INDEX_UNREADABLE {
            write!(
                f,
                "; the index file exists but could not be read — repair or \
                 remove it and re-index, or pass --no-jcodemunch to skip the \
                 jcodemunch-backed detectors"
            )
        } else {
            write!(
                f,
                "; re-index this checkout before querying, or pass --no-jcodemunch \
                 to skip the jcodemunch-backed detectors"
            )
        }
    }
}

/// Decide whether `state` is a corpus worth querying, per §4.3.
///
/// Four-arm ladder, in this order — the order is the contract, pinned by
/// tests, not an artifact of statement sequence:
///
/// 0. `unreadable` set ⇒ [`E_JC_INDEX_UNREADABLE`]. Evaluated FIRST because
///    every quantity below it was read from a file we could not fully read,
///    so any verdict derived from them would be an assertion about a corpus
///    we never saw. It is also the arm with a different remedy.
/// 1. no `index_head` ⇒ [`E_JC_INDEX_EMPTY`]. An absent index is the limiting
///    case of an empty one; it is classified EMPTY rather than STALE because
///    there is no head to name and the refusal is required to name it.
/// 2. `index_head != live_head` ⇒ [`E_JC_INDEX_STALE`]. Queries answer about
///    a different commit's code, so P1 reports symbols as orphaned that the
///    current tree references.
/// 3. no symbols ⇒ [`E_JC_INDEX_EMPTY`]. The mirror image: every query
///    returns nothing, so every producer looks orphaned.
///
/// Identity-agnostic — the caller attaches the probed `repo_id` via
/// [`IndexRefusal::with_repo_id`] and the exact count via
/// [`IndexRefusal::with_symbol_count`] — so the decision stays a pure
/// function of the state it is handed, with no filesystem access of its own.
pub fn evaluate_freshness(state: &IndexState, live_head: &str) -> Result<(), IndexRefusal> {
    let refuse = |code: &'static str| IndexRefusal {
        code,
        repo_id: String::new(),
        index_head: state.index_head.clone(),
        live_head: live_head.to_string(),
        // 0 by construction — see `with_symbol_count`.
        symbol_count: 0,
        // Propagated here rather than re-attached by the caller: a diagnostic
        // that only survives because one call site remembers to forward it is
        // one refactor away from vanishing silently.
        unreadable: state.unreadable.clone(),
    };

    if state.unreadable.is_some() {
        return Err(refuse(E_JC_INDEX_UNREADABLE));
    }
    match &state.index_head {
        None => Err(refuse(E_JC_INDEX_EMPTY)),
        Some(head) if head != live_head => Err(refuse(E_JC_INDEX_STALE)),
        Some(_) if !state.symbols_present => Err(refuse(E_JC_INDEX_EMPTY)),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
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
        assert!(state.symbols_present, "3 rows is a non-empty corpus");
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
        assert!(!state.symbols_present, "an empty husk carries no symbols");
        assert_eq!(state.unreadable, None, "a husk is readable, just empty");
    }

    #[test]
    fn read_index_state_on_absent_db_reports_absence_without_creating_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let expected = index_db_path(tmp.path(), "local/proj-deadbeef");
        assert!(!expected.exists(), "precondition: no index db yet");

        let state = read_index_state(tmp.path(), "local/proj-deadbeef");
        assert_eq!(state.index_head, None);
        assert!(!state.symbols_present);
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
        assert!(
            state.symbols_present,
            "the symbols probe is read independently of the meta probe"
        );
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
        assert!(!state.symbols_present);
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

    /// A readable `IndexState` with the §4.3 quantities set explicitly.
    fn state(index_head: Option<&str>, symbols_present: bool) -> IndexState {
        IndexState {
            index_head: index_head.map(str::to_string),
            symbols_present,
            unreadable: None,
        }
    }

    /// An `IndexState` that could not be read — the file is there, the
    /// contents are not trustworthy.
    fn unreadable_state(diag: &str) -> IndexState {
        IndexState {
            unreadable: Some(diag.to_string()),
            ..IndexState::absent()
        }
    }

    const LIVE: &str = "1111111111111111111111111111111111111111";
    const OTHER: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn evaluate_freshness_admits_a_fresh_populated_index() {
        assert!(
            evaluate_freshness(&state(Some(LIVE), true), LIVE).is_ok(),
            "an index at the live commit with symbols in it must be admitted"
        );
    }

    #[test]
    fn evaluate_freshness_refuses_a_stale_index() {
        let refusal = evaluate_freshness(&state(Some(OTHER), true), LIVE)
            .expect_err("an index at a different commit must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_STALE);
    }

    #[test]
    fn evaluate_freshness_refuses_an_empty_husk_index() {
        // Fresh head, zero symbols: the mirror-image failure. Every query
        // returns nothing, so P1 reports every producer as an orphan while
        // the head comparison alone would have said "all good".
        let refusal = evaluate_freshness(&state(Some(LIVE), false), LIVE)
            .expect_err("an index with zero symbols must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_EMPTY);
    }

    #[test]
    fn evaluate_freshness_refuses_an_absent_index_as_empty() {
        // Precedence arm 1. Classified EMPTY, not STALE: there is no
        // index_head to name, and §4.3 requires the refusal to name it —
        // calling a nonexistent head "stale" would be nonsense.
        let refusal = evaluate_freshness(&state(None, false), LIVE)
            .expect_err("an absent index must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_EMPTY);
    }

    #[test]
    fn evaluate_freshness_resolves_both_degenerate_to_stale() {
        // Heads differ AND the corpus is empty. Pinning STALE here fixes
        // §4.3's left-to-right conjunct order, so the outcome is deterministic
        // rather than an incidental consequence of statement order.
        let refusal = evaluate_freshness(&state(Some(OTHER), false), LIVE)
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
        // The exact count is attached by the caller (`with_symbol_count`,
        // fed by `count_symbols`) rather than read on the startup path — see
        // `IndexState::symbols_present`. Rendering it is still part of §4.3's
        // prose requirement, so the builder is exercised here too.
        let stale = evaluate_freshness(&state(Some(OTHER), true), LIVE)
            .unwrap_err()
            .with_symbol_count(42);
        let msg = stale.to_string();
        assert!(
            msg.contains(E_JC_INDEX_STALE),
            "code token missing from {msg}"
        );
        assert!(msg.contains(OTHER), "index_head missing from {msg}");
        assert!(msg.contains(LIVE), "live_head missing from {msg}");
        assert!(msg.contains("42"), "symbol_count missing from {msg}");

        let empty = evaluate_freshness(&state(Some(LIVE), false), LIVE).unwrap_err();
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
        let refusal = evaluate_freshness(&state(None, false), LIVE).unwrap_err();
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
    fn the_marker_codes_are_distinct_non_empty_tokens() {
        // A refactor that collapsed any two of these to one token, or emptied
        // one, would silently destroy every machine consumer's ability to
        // tell a stale corpus from an absent one from an unreadable one while
        // all the code() assertions above still passed.
        let codes = [E_JC_INDEX_STALE, E_JC_INDEX_EMPTY, E_JC_INDEX_UNREADABLE];
        for (i, a) in codes.iter().enumerate() {
            assert!(!a.is_empty(), "marker {i} is empty");
            for b in &codes[i + 1..] {
                assert_ne!(a, b, "marker codes must stay distinct");
            }
        }
    }

    #[test]
    fn evaluate_freshness_refuses_an_unreadable_index_with_its_own_code() {
        // Precedence arm 0. An unreadable file must NOT be collapsed into
        // E_JC_INDEX_EMPTY: the corpus behind a WAL database whose watcher is
        // down, or behind a permissions fault, is intact, and "re-index this
        // checkout" is then the wrong instruction — a full re-index that
        // learns nothing.
        let refusal = evaluate_freshness(&unreadable_state("disk I/O error"), LIVE)
            .expect_err("an unreadable index must be refused");
        assert_eq!(refusal.code(), E_JC_INDEX_UNREADABLE);
    }

    #[test]
    fn unreadable_refusal_outranks_a_head_mismatch() {
        // Every quantity below arm 0 was read from a file we could not fully
        // read, so a STALE verdict derived from them would be an assertion
        // about a corpus we never saw.
        let drifted = IndexState {
            index_head: Some(OTHER.to_string()),
            symbols_present: true,
            unreadable: Some("no such table: symbols".to_string()),
        };
        let refusal = evaluate_freshness(&drifted, LIVE).expect_err("must refuse");
        assert_eq!(
            refusal.code(),
            E_JC_INDEX_UNREADABLE,
            "unreadability is evaluated before staleness"
        );
    }

    #[test]
    fn unreadable_refusal_carries_the_diagnostic_and_its_own_remedy() {
        // The diagnostic is propagated by `evaluate_freshness` itself, not
        // re-attached by the caller: a diagnostic that survives only because
        // one call site remembers to forward it is one refactor from
        // vanishing silently.
        let refusal =
            evaluate_freshness(&unreadable_state("file is not a database"), LIVE).unwrap_err();
        let msg = refusal.to_string();
        assert!(
            msg.contains("index unreadable: file is not a database"),
            "the reader's diagnostic must survive into the message, got {msg}"
        );
        assert!(
            !msg.contains("re-index this checkout before querying"),
            "the empty/stale remedy points at the wrong artifact here, got {msg}"
        );
        assert!(
            msg.contains("could not be read"),
            "the remedy must say the file could not be read, got {msg}"
        );
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

    // -------------------------------------------------------------------
    // §4.2 — agreement with the indexer's own normalization
    //
    // `scripts/jcodemunch-index-reify.sh` derives the SAME id in bash, and it
    // is the side that actually writes the index. Any divergence makes the
    // gate probe a file the indexer never created, so the refusal names an
    // identity corresponding to nothing and sends the operator to re-index a
    // phantom. These pin the three spellings where the two implementations
    // used to disagree.
    // -------------------------------------------------------------------

    /// The script's pipeline: `~`-expand, then `readlink -f`. Computed by
    /// invoking the same coreutils the script invokes, so this is a genuine
    /// cross-implementation check rather than our own logic restated.
    fn readlink_f(path: &str) -> String {
        let home = std::env::var("HOME").expect("HOME is required to mirror the script");
        let expanded = if path == "~" {
            home
        } else if let Some(rest) = path.strip_prefix("~/") {
            format!("{home}/{rest}")
        } else {
            path.to_string()
        };
        let out = std::process::Command::new("readlink")
            .args(["-f", "--", &expanded])
            .output()
            .expect("readlink -f");
        assert!(out.status.success(), "readlink -f failed for {expanded}");
        String::from_utf8(out.stdout)
            .expect("utf8 path")
            .trim_end()
            .to_string()
    }

    #[test]
    fn resolve_repo_id_agrees_with_the_indexer_scripts_readlink_f_semantics() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        // An existing dir; a missing LEAF under an existing parent (where
        // `readlink -f` is non-strict and plain `canonicalize` fails); a
        // relative path; and an unexpanded `~` form.
        let cases = [
            manifest.to_string(),
            format!("{manifest}/no-such-dir-6108"),
            "src".to_string(),
            "~/no-such-dir-6108".to_string(),
        ];
        for case in cases {
            let expected = repo_id_for_abs_path(Path::new(&readlink_f(&case)));
            assert_eq!(
                resolve_repo_id(Path::new(&case)),
                expected,
                "identity for {case:?} must match the indexer script's derivation"
            );
        }
    }

    #[test]
    fn resolve_repo_id_expands_a_leading_tilde() {
        // A `--project-root` arriving quoted from a config or another script
        // is never expanded by a shell. Hashing the literal `~/...` string
        // would derive an id no indexer could ever have written.
        let home = std::env::var("HOME").expect("HOME is required by the derivation under test");
        assert_eq!(
            resolve_repo_id(Path::new("~/no-such-dir-6108")),
            resolve_repo_id(Path::new(&format!("{home}/no-such-dir-6108"))),
            "a leading ~ must expand to $HOME"
        );
        // `~user/…` is deliberately NOT expanded — the script does not expand
        // it either, so guessing here would introduce a divergence.
        let literal = resolve_repo_id(Path::new("~someone/no-such-dir-6108"));
        assert!(
            literal.starts_with("local/"),
            "a ~user path still derives an id, got {literal}"
        );
    }

    #[test]
    fn resolve_repo_id_absolutizes_a_relative_root_that_does_not_exist() {
        // The old fallback hashed the string AS GIVEN whenever canonicalize
        // failed, so a relative root under a nonexistent leaf derived an id
        // from `"no-such-dir-6108"` rather than from its absolute path.
        let cwd = std::env::current_dir().expect("cwd");
        assert_eq!(
            resolve_repo_id(Path::new("no-such-dir-6108")),
            resolve_repo_id(&cwd.join("no-such-dir-6108")),
            "a relative root must be joined onto the current directory"
        );
    }

    #[test]
    fn read_index_state_reads_a_wal_index_with_no_shm_and_a_read_only_directory() {
        // jcodemunch's index is maintained by a long-running watcher. Where
        // that watcher runs under another account (or the index dir is a
        // read-only mount), the DB can be left in WAL mode with an
        // uncheckpointed `-wal`, no `-shm`, and no write access to create one.
        // Measured on the bundled SQLite 3.45: the read-only OPEN succeeds and
        // the first QUERY fails with "unable to open database file". The corpus
        // is intact, so refusing with "re-index this checkout" would send the
        // operator to rebuild the wrong thing.
        let tmp = tempfile::tempdir().expect("tempdir");
        let staging = tmp.path().join("staging");
        let index_dir = tmp.path().join("index");
        std::fs::create_dir_all(&index_dir).expect("create index dir");

        let staged = write_index_db(&staging, "local/proj-deadbeef", Some("abc123"), 4);
        let live = index_dir.join("local-proj-deadbeef.db");
        {
            let conn = rusqlite::Connection::open(&staged).expect("reopen to switch journal mode");
            let mode: String = conn
                .query_row("pragma journal_mode=WAL", [], |row| row.get(0))
                .expect("set WAL");
            assert_eq!(mode.to_lowercase(), "wal", "fixture must be in WAL mode");
            conn.execute(
                "INSERT INTO symbols (name, path) VALUES ('wal_row', 'src/lib.rs')",
                [],
            )
            .expect("insert under WAL");
            // Copy db + -wal WHILE the writer still holds them, so the copy
            // carries an uncheckpointed WAL and no -shm. Closing first would
            // let SQLite checkpoint and delete both sidecars, which is the
            // healthy shape this test is deliberately not about.
            std::fs::copy(&staged, &live).expect("copy main db");
            std::fs::copy(
                PathBuf::from(format!("{}-wal", staged.display())),
                PathBuf::from(format!("{}-wal", live.display())),
            )
            .expect("copy -wal");
        }

        // Permissions are restored before any assertion so a failure here
        // cannot leave an unremovable tempdir behind.
        std::fs::set_permissions(&index_dir, std::fs::Permissions::from_mode(0o555))
            .expect("chmod index dir read-only");
        let plain_read_failed = rusqlite::Connection::open_with_flags(
            &live,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .and_then(|c| c.query_row("select count(*) from symbols", [], |r| r.get::<_, i64>(0)))
        .is_err();
        let state = read_index_state(&index_dir, "local/proj-deadbeef");
        let lazy_count = count_symbols(&index_dir, "local/proj-deadbeef");
        std::fs::set_permissions(&index_dir, std::fs::Permissions::from_mode(0o755))
            .expect("restore index dir permissions");

        // PREMISE: without the fallback this read is what fails. If this
        // assertion ever starts failing, the fallback is no longer exercised
        // here and this test has quietly stopped proving anything.
        assert!(
            plain_read_failed,
            "a plain read-only read of a WAL db with an uncheckpointed -wal, no -shm \
             and no writable directory must fail"
        );

        assert_eq!(
            state.unreadable, None,
            "such an index is readable via the immutable fallback, not a fault"
        );
        assert_eq!(state.index_head.as_deref(), Some("abc123"));
        assert!(state.symbols_present, "the corpus is intact");
        // `immutable=1` reads the main database and ignores WAL frames, so the
        // row written after the mode switch is not visible. That is the
        // documented trade-off: the fallback answers §4.3's two questions from
        // the last checkpointed image, and any error is in the fails-closed
        // direction.
        assert_eq!(
            lazy_count, 4,
            "the lazy exact count reads the same checkpointed image"
        );
    }

    #[test]
    fn count_symbols_is_zero_for_an_absent_index_and_does_not_create_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let expected = index_db_path(tmp.path(), "local/proj-deadbeef");
        assert_eq!(count_symbols(tmp.path(), "local/proj-deadbeef"), 0);
        assert!(
            !expected.exists(),
            "the lazy count must not create the db it fails to find"
        );
    }

    #[test]
    fn count_symbols_reports_the_exact_row_count() {
        // The number `read_index_state` deliberately does not pay for. It has
        // exactly one consumer — the rendered refusal — so it is pinned here
        // rather than on the startup path.
        let tmp = tempfile::tempdir().expect("tempdir");
        write_index_db(tmp.path(), "local/proj-deadbeef", Some("abc123"), 9);
        assert_eq!(count_symbols(tmp.path(), "local/proj-deadbeef"), 9);
    }
}
