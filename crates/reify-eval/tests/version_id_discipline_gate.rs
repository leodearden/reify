//! Version-id discipline gate (task ε, PRD
//! `docs/prds/v0_6/engine-build-hardening.md` §8 / D7) — makes INV-BUILD-2
//! (`docs/invariants.md`: "Version/snapshot IDs are allocated and read
//! through exactly one API each") self-enforcing.
//!
//! Scans `crates/reify-eval/src/**/*.rs` for raw `self.next_version_id` /
//! `self.next_snapshot_id` arithmetic or reads outside the allocator
//! (`Engine::allocate_snapshot_version`, engine_admin.rs) and the two named
//! readers (`Engine::last_allocated_version`, engine_admin.rs;
//! `Engine::current_eval_version`, engine_build.rs) — the only fns where
//! bumping/reading the raw counters IS the API's own definition. A line
//! carrying an explicit `// version-id-gate: allow — <reason>` escape
//! comment is exempted (same grammar as the `ptodo:allow` precedent).
//!
//! Step-1 laid down anti-vacuous unit self-tests over `scan_source` before
//! it existed (mirroring the seeded-violation self-test pattern in
//! `no_stale_undef_invariant_gate.rs::seeded_stale_undef_violation_is_reported`);
//! step-2 (below) implements the matcher so they pass. Step-3 adds the
//! real-tree gate test that walks `crates/reify-eval/src/` itself.

/// One raw, non-exempt `self.next_version_id` / `self.next_snapshot_id` use
/// found outside the allocator/reader API family.
#[derive(Debug)]
struct Violation {
    /// The label passed to `scan_source` — the scanned file's display path
    /// (real-tree scans) or bare basename (unit self-tests below).
    file: String,
    /// 1-based line number within the scanned source.
    line: usize,
    /// The raw (not comment-stripped) source line, trimmed.
    text: String,
}

// ── Step-2: the matcher ──────────────────────────────────────────────────────

/// Escape-hatch comment token (same grammar as the `ptodo:allow`
/// precedent) that exempts a line from the gate regardless of its
/// enclosing fn. Matched against the RAW line, before comment-stripping —
/// the token itself lives inside a `//` comment.
const ALLOW_COMMENT: &str = "version-id-gate: allow";

/// Raw `self.`-scoped counter tokens the gate bans outside the allocate/read
/// API family's own definitions.
const TOKENS: &[&str] = &["self.next_version_id", "self.next_snapshot_id"];

/// The only (file basename, fn name) pairs where a raw `self.next_*_id` use
/// IS the allocate/read API's own definition: the allocator
/// (`allocate_snapshot_version`) and the two named readers
/// (`last_allocated_version`, `current_eval_version`) — see `docs/invariants.md`
/// INV-BUILD-2. Allowlisting by enclosing fn (not whole file) means a new
/// raw bump added anywhere ELSE in engine_admin.rs/engine_build.rs is still
/// caught.
const ALLOWED_FNS: &[(&str, &str)] = &[
    ("engine_admin.rs", "allocate_snapshot_version"),
    ("engine_admin.rs", "last_allocated_version"),
    ("engine_build.rs", "current_eval_version"),
];

/// Naively strips a `//` line comment: cuts at the first `//`, keeping only
/// what precedes it. Documented limitation: this doesn't understand
/// string/char literals, so a `//` inside one would be (incorrectly)
/// treated as a comment start — acceptable for a source-text lint gate,
/// not a full Rust parser; none of the scanned production call sites embed
/// `//` inside a same-line string literal alongside a counter token.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Parses the fn name out of a (trimmed) line that looks like a fn
/// signature, stripping leading `pub`/`pub(...)`/`async`/`unsafe`
/// modifiers first. Returns `None` if the line isn't a fn signature.
fn parse_fn_name(trimmed: &str) -> Option<String> {
    let mut rest = trimmed;
    loop {
        if let Some(r) = rest.strip_prefix("pub(") {
            match r.find(')') {
                Some(idx) => {
                    rest = r[idx + 1..].trim_start();
                    continue;
                }
                None => return None,
            }
        }
        if let Some(r) = rest.strip_prefix("pub ") {
            rest = r.trim_start();
            continue;
        }
        if let Some(r) = rest.strip_prefix("async ") {
            rest = r.trim_start();
            continue;
        }
        if let Some(r) = rest.strip_prefix("unsafe ") {
            rest = r.trim_start();
            continue;
        }
        break;
    }
    let rest = rest.strip_prefix("fn ")?;
    let rest = rest.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_string())
}

/// Returns the basename component of `label` (e.g. `"engine_admin.rs"` out
/// of a full path) for the `ALLOWED_FNS` lookup. `label` itself (not the
/// extracted basename) is stored verbatim in `Violation::file`, so
/// real-tree scans can pass a full display path for useful diagnostics.
fn basename(label: &str) -> &str {
    std::path::Path::new(label)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(label)
}

/// Scans `source` (the contents of a file identified by `label`) for raw
/// `self.next_version_id` / `self.next_snapshot_id` uses outside the
/// allocate/read API family (`ALLOWED_FNS`), honouring the
/// `// version-id-gate: allow` escape comment and ignoring comment-only
/// mentions. See the module doc comment for the full contract.
///
/// The enclosing-fn allowlist is scoped by brace depth: `current_fn` is
/// cleared once we leave its body (the net `{`/`}` depth returns to the
/// signature's level after having entered it), so an allowlisted fn's
/// exemption cannot leak onto a later raw use that merely precedes the next
/// `fn` name `parse_fn_name` recovers (e.g. a signature whose `fn` keyword
/// and name are split across lines).
fn scan_source(label: &str, source: &str) -> Vec<Violation> {
    let base = basename(label);
    let mut current_fn: Option<String> = None;
    // Net `{`-minus-`}` depth at which `current_fn`'s signature appeared, and
    // whether we have since descended into its body. Together they let us
    // clear the exemption on leaving the body rather than leaving it sticky
    // until the next parsed signature.
    let mut fn_depth: usize = 0;
    let mut in_body = false;
    let mut depth: usize = 0;
    let mut violations = Vec::new();

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let stripped = strip_line_comment(raw_line);

        if let Some(name) = parse_fn_name(raw_line.trim()) {
            current_fn = Some(name);
            fn_depth = depth;
            in_body = false;
        }

        // The allow token is matched against the RAW line (pre-strip): it
        // lives inside the very `//` comment `strip_line_comment` removes.
        if !raw_line.contains(ALLOW_COMMENT) && TOKENS.iter().any(|tok| stripped.contains(tok)) {
            let allowed = current_fn
                .as_deref()
                .is_some_and(|f| ALLOWED_FNS.contains(&(base, f)));
            if !allowed {
                violations.push(Violation {
                    file: label.to_string(),
                    line: line_no,
                    text: raw_line.trim().to_string(),
                });
            }
        }

        // Advance brace depth on the comment-stripped line, then clear the
        // enclosing-fn exemption once we return to its signature depth after
        // having descended into the body. Handling entry separately keeps a
        // split signature (whose `{` lands on a later line than the name)
        // from clearing the fn before its body is even reached.
        for c in stripped.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if current_fn.is_some() {
            if depth > fn_depth {
                in_body = true;
            } else if in_body {
                current_fn = None;
                in_body = false;
            }
        }
    }

    violations
}

// ── Step-1 self-tests (now GREEN) ────────────────────────────────────────────

/// (a) A synthetic raw bump inside a non-allowlisted fn body is reported —
/// the permanent "demonstrably RED if reintroduced" proof, mirroring
/// `seeded_stale_undef_violation_is_reported`.
#[test]
fn seeded_synthetic_violation_is_reported() {
    let source = "\
impl Engine {
    pub fn some_other_method(&mut self) {
        self.next_version_id += 1;
    }
}
";
    let violations = scan_source("engine_eval.rs", source);
    assert_eq!(
        violations.len(),
        1,
        "expected exactly one violation for a raw bump reintroduced in a \
         non-allowlisted fn, got {violations:?}"
    );
    assert_eq!(violations[0].file, "engine_eval.rs");
    assert_eq!(
        violations[0].line, 3,
        "violation should point at the `self.next_version_id += 1;` line, got {:?}",
        violations[0]
    );
    assert!(
        violations[0].text.contains("next_version_id"),
        "violation text should quote the offending line, got {:?}",
        violations[0].text
    );
}

/// (b) The SAME token inside each allowlisted fn's own body is not
/// reported — these three fns ARE the allocate/read API family.
#[test]
fn allowlisted_fn_bodies_report_zero_violations() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "engine_admin.rs",
            "allocate_snapshot_version",
            "self.next_snapshot_id += 1;\n        self.next_version_id += 1;",
        ),
        (
            "engine_admin.rs",
            "last_allocated_version",
            "self.next_version_id.saturating_sub(1)",
        ),
        (
            "engine_build.rs",
            "current_eval_version",
            "self.next_version_id",
        ),
    ];
    for (label, fn_name, body) in cases {
        let source = format!(
            "impl Engine {{\n    pub(crate) fn {fn_name}(&mut self) -> VersionId {{\n        {body}\n    }}\n}}\n"
        );
        let violations = scan_source(label, &source);
        assert!(
            violations.is_empty(),
            "expected zero violations inside allowlisted fn {fn_name} \
             ({label}), got {violations:?}"
        );
    }
}

/// (c) A line carrying the `// version-id-gate: allow — <reason>` escape
/// comment is exempted even inside a non-allowlisted fn.
#[test]
fn allow_comment_escape_hatch_suppresses_violation() {
    let source = "\
impl Engine {
    fn restore_from_setup(&mut self, setup: &Setup) {
        self.next_snapshot_id = setup.snapshot_id.0; // version-id-gate: allow — setup restore, not allocation
        self.next_version_id = setup.version.0; // version-id-gate: allow — setup restore, not allocation
    }
}
";
    let violations = scan_source("concurrent.rs", source);
    assert!(
        violations.is_empty(),
        "expected the allow-comment escape hatch to suppress both lines, \
         got {violations:?}"
    );
}

/// (d) A comment-only (`//` or `///`) mention of the token is not matched —
/// the gate matches code tokens, not prose.
#[test]
fn comment_only_mentions_are_not_matched() {
    let source = "\
impl Engine {
    /// Must not read `self.next_version_id` directly — use the allocator.
    // self.next_snapshot_id is documented here too.
    fn some_fn(&mut self) {}
}
";
    let violations = scan_source("engine_build.rs", source);
    assert!(
        violations.is_empty(),
        "expected doc/comment-only mentions to be stripped before \
         matching, got {violations:?}"
    );
}

/// (e) A bare (non-`self.`-scoped) constructor initialiser and an
/// `engine.`-scoped test-style read are not matched — the gate is scoped to
/// the Engine's own `&mut self` allocation idiom, not every appearance of
/// the token.
#[test]
fn non_self_scoped_uses_are_not_matched() {
    let source = "\
impl Engine {
    fn new() -> Self {
        Self {
            next_snapshot_id: 0,
            next_version_id: 0,
        }
    }
}

fn read_counters(engine: &Engine) -> (u64, u64) {
    (engine.next_snapshot_id, engine.next_version_id)
}
";
    let violations = scan_source("engine_admin.rs", source);
    assert!(
        violations.is_empty(),
        "expected non-`self.`-scoped uses (constructor initialiser, \
         `engine.`-bound test read) to be ignored, got {violations:?}"
    );
}

/// (f) Regression guard for enclosing-fn exemption leakage: a raw bump
/// placed after an allowlisted fn's body — but before the next fn whose name
/// `parse_fn_name` can recover (here the `fn` keyword and the name are split
/// across lines) — must NOT inherit the preceding fn's exemption. Before
/// `scan_source` scoped the allowlist by brace depth this slipped through as
/// a silent false negative inside `engine_admin.rs` / `engine_build.rs`.
#[test]
fn allowlisted_fn_exemption_does_not_leak_past_its_body() {
    let source = "\
impl Engine {
    pub(crate) fn allocate_snapshot_version(&mut self) -> (SnapshotId, VersionId) {
        self.next_snapshot_id += 1;
        self.next_version_id += 1;
        (SnapshotId(self.next_snapshot_id), VersionId(self.next_version_id))
    }

    pub(crate) fn
        sneaky_reintroduction(&mut self) {
        self.next_version_id += 1;
    }
}
";
    let violations = scan_source("engine_admin.rs", source);
    assert_eq!(
        violations.len(),
        1,
        "the raw bump in `sneaky_reintroduction` must not inherit \
         `allocate_snapshot_version`'s exemption once its body has closed, \
         got {violations:?}"
    );
    assert!(
        violations[0].text.contains("next_version_id"),
        "the reported violation should quote the sneaky raw bump, got {:?}",
        violations[0]
    );
}

// ── Step-3: real-tree gate ───────────────────────────────────────────────────

/// Recursively collect every `.rs` file under `dir` (including
/// subdirectories). Unreadable entries/directories are silently skipped —
/// this only ever walks our own crate's `src/` directory, which is
/// expected to be readable. Mirrors `no_stale_undef_invariant_gate.rs`'s
/// `collect_ri_files`, filtering `.rs` instead of `.ri`.
fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The real-tree debug gate for INV-BUILD-2 (`docs/invariants.md`): every
/// `.rs` file under `crates/reify-eval/src/` must be free of raw
/// `self.next_version_id` / `self.next_snapshot_id` uses outside the
/// allocate/read API family — see the module doc comment.
#[test]
fn no_raw_version_id_use_outside_allocator() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);

    let mut violations: Vec<Violation> = Vec::new();
    for path in &files {
        let display = path.display().to_string();
        let source =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {display}: {e}"));
        violations.extend(scan_source(&display, &source));
    }

    assert!(
        violations.is_empty(),
        "found {} raw next_version_id/next_snapshot_id use(s) outside the \
         allocate/read API family (docs/invariants.md INV-BUILD-2) — route \
         through Engine::allocate_snapshot_version, or add a `// \
         version-id-gate: allow — <reason>` comment if this is a \
         deliberate, non-allocating exception:\n{}",
        violations.len(),
        violations
            .iter()
            .map(|v| format!("  {}:{}: {}", v.file, v.line, v.text))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
