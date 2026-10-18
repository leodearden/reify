//! Inline-pattern ratchet over `crates/reify-kernel-occt/src` (INV candidate 5;
//! PRD `docs/prds/v0_6/occt-crate-internals.md` D9).
//!
//! Each row counts one open-coded construct in the PRODUCTION view of the
//! crate's sources and holds it to a committed baseline. The rule is
//! one-directional: live ≤ baseline. A new instance needs a visible baseline
//! edit in the same diff; there is no allow-comment escape hatch. Later leaves
//! (γ2, ε, θ, δ) add rows here. Equal validation messages across entry points
//! are a behavioural property, guarded by `op_args_entry_point_parity.rs`, not
//! a row here. γ2's rows (iv)–(vi) share one homed-uses mechanism: named
//! identifiers used only inside their home fns, every home seen using one.
//!
//! The production view comes from a whole-file lexer modelled on
//! `crates/reify-builtins/tests/common/seed_name_scan.rs` (test-private to that
//! crate, so ported rather than called). Enclosing-fn scoping follows
//! `crates/reify-eval/tests/version_id_discipline_gate.rs`. Folding this lexer
//! and its siblings onto one shared production view in `reify-test-support`,
//! leaving only the rows here, is task #8796.

use std::path::{Path, PathBuf};

// ── production view ─────────────────────────────────────────────────────────

/// A string literal (plain, byte or raw) found at code level.
struct StrLit {
    /// Byte offset of the literal's first byte: its quote or `r`/`b` prefix.
    start: usize,
    /// Byte offset one past its closing quote and any raw-string `#`s.
    end: usize,
    /// Byte offset of the first content byte.
    content_start: usize,
    /// The content as written, escapes unprocessed.
    content: String,
}

/// One source file as production code sees it.
struct ProductionView {
    /// The source with comments, char literals and cfg(test)-gated items
    /// blanked to spaces. Newlines are kept, so byte offsets and line numbers
    /// both survive; string literals outside masked items stay intact.
    code: Vec<u8>,
    literals: Vec<StrLit>,
}

/// One source file as the lexer reads it, before any item is masked.
fn lexed_view(src: &str) -> ProductionView {
    let (code, literals) = lex(src);
    ProductionView { code, literals }
}

fn production_view(src: &str) -> ProductionView {
    let mut view = lexed_view(src);
    mask_test_gated_items(&mut view.code, &view.literals);
    view
}

impl ProductionView {
    /// `code` with string-literal contents blanked too, one entry per source
    /// line (index 0 is line 1).
    fn skeleton_lines(&self) -> Vec<String> {
        let mut skeleton = self.code.clone();
        for lit in &self.literals {
            blank(
                &mut skeleton,
                lit.content_start,
                lit.content_start + lit.content.len(),
            );
        }
        String::from_utf8_lossy(&skeleton)
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// Blank `bytes[a..b]` to spaces, keeping newlines.
fn blank(bytes: &mut [u8], a: usize, b: usize) {
    let end = b.min(bytes.len());
    for byte in &mut bytes[a.min(end)..end] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn starts_token(raw: &[u8], at: usize) -> bool {
    at == 0 || !is_ident_byte(raw[at - 1])
}

/// One past the closing `'` when a char literal starts at `at`, or `None` when
/// the quote opens a lifetime (`'static`, `&'a str`).
fn char_literal_end(raw: &[u8], at: usize) -> Option<usize> {
    let n = raw.len();
    let first = at + 1;
    if first >= n {
        return None;
    }
    if raw[first] == b'\\' {
        let mut j = first + 2;
        while j < n && raw[j] != b'\'' {
            j += 1;
        }
        return (j < n).then_some(j + 1);
    }
    let width = match raw[first] {
        b if b < 0x80 => 1,
        b if b >> 5 == 0b110 => 2,
        b if b >> 4 == 0b1110 => 3,
        _ => 4,
    };
    let close = first + width;
    (close < n && raw[close] == b'\'').then_some(close + 1)
}

/// The plain (or byte) string literal whose opening quote is at `quote`.
fn plain_string_at(src: &str, quote: usize) -> StrLit {
    let raw = src.as_bytes();
    let content_start = quote + 1;
    let mut i = content_start;
    while i < raw.len() && raw[i] != b'"' {
        i += if raw[i] == b'\\' { 2 } else { 1 };
    }
    let content_end = i.min(raw.len());
    StrLit {
        start: quote,
        end: (content_end + 1).min(raw.len()),
        content_start,
        content: src[content_start..content_end].to_owned(),
    }
}

/// The raw string literal (`r"…"`, `r#"…"#`, `br"…"`) starting at `at`, if
/// one does.
fn raw_string_at(src: &str, at: usize) -> Option<StrLit> {
    let raw = src.as_bytes();
    let mut j = at;
    if raw[j] == b'b' {
        j += 1;
    }
    if raw.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = raw[j..].iter().take_while(|&&b| b == b'#').count();
    j += hashes;
    if raw.get(j) != Some(&b'"') {
        return None;
    }
    let content_start = j + 1;
    let closing: Vec<u8> = std::iter::once(b'"')
        .chain(std::iter::repeat_n(b'#', hashes))
        .collect();
    let content_end = raw[content_start..]
        .windows(closing.len())
        .position(|w| w == closing.as_slice())
        .map_or(raw.len(), |p| content_start + p);
    Some(StrLit {
        start: at,
        end: (content_end + closing.len()).min(raw.len()),
        content_start,
        content: src[content_start..content_end].to_owned(),
    })
}

/// Blank every comment (line, doc, nested block) and char literal, and collect
/// every string literal. State carries across lines, so a backslash-continued
/// string stays a string on every line it spans.
fn lex(src: &str) -> (Vec<u8>, Vec<StrLit>) {
    let raw = src.as_bytes();
    let n = raw.len();
    let mut code = raw.to_vec();
    let mut literals = Vec::new();
    let mut i = 0usize;
    while i < n {
        match raw[i] {
            b'/' if raw.get(i + 1) == Some(&b'/') => {
                let start = i;
                while i < n && raw[i] != b'\n' {
                    i += 1;
                }
                blank(&mut code, start, i);
            }
            b'/' if raw.get(i + 1) == Some(&b'*') => {
                let start = i;
                let mut depth = 0usize;
                while i < n {
                    if raw[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if raw[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                blank(&mut code, start, i);
            }
            b'r' | b'b' if starts_token(raw, i) => match raw_string_at(src, i) {
                Some(lit) => {
                    i = lit.end;
                    literals.push(lit);
                }
                None => i += 1,
            },
            b'"' => {
                let lit = plain_string_at(src, i);
                i = lit.end;
                literals.push(lit);
            }
            b'\'' => match char_literal_end(raw, i) {
                Some(end) => {
                    blank(&mut code, i, end);
                    i = end;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    (code, literals)
}

/// Does this `#[cfg(…)]` attribute gate test-only code? The same
/// classification as `seed_name_scan.rs`'s `attr_gates_test_code`: the `test`
/// ident not under `not(…)`, or a `feature = "test…"`.
fn attr_gates_test_code(attr: &str) -> bool {
    let Some(inner) = attr.strip_prefix("#[cfg(") else {
        return false;
    };
    let b = inner.as_bytes();
    let mut not_depths: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    let mut last_ident = "";
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                while not_depths.last() == Some(&depth) {
                    not_depths.pop();
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'"' => {
                let start = i + 1;
                let mut k = start;
                while k < b.len() && b[k] != b'"' {
                    if b[k] == b'\\' {
                        k += 1;
                    }
                    k += 1;
                }
                let content = &inner[start..k.min(inner.len())];
                if last_ident == "feature" && content.starts_with("test") && not_depths.is_empty() {
                    return true;
                }
                last_ident = "";
                i = k + 1;
            }
            c if is_ident_byte(c) => {
                let start = i;
                while i < b.len() && is_ident_byte(b[i]) {
                    i += 1;
                }
                let ident = &inner[start..i];
                if ident == "not" {
                    not_depths.push(depth + 1);
                } else if ident == "test" && not_depths.is_empty() {
                    return true;
                }
                last_ident = ident;
            }
            _ => i += 1,
        }
    }
    false
}

fn literal_mask(len: usize, literals: &[StrLit]) -> Vec<bool> {
    let mut mask = vec![false; len + 1];
    for lit in literals {
        for slot in mask.iter_mut().take(lit.end.min(len)).skip(lit.start) {
            *slot = true;
        }
    }
    mask
}

/// Offset of the `}` matching the `{` at `open` (or the end of the buffer).
fn matching_brace(code: &[u8], in_literal: &[bool], open: usize) -> usize {
    let mut depth = 0usize;
    for (k, &byte) in code.iter().enumerate().skip(open) {
        if in_literal[k] {
            continue;
        }
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    code.len().saturating_sub(1)
}

/// Offset of the `]` closing the attribute that opens at `hash`.
fn attribute_end(code: &[u8], in_literal: &[bool], hash: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, &byte) in code.iter().enumerate().skip(hash + 1) {
        if in_literal[j] {
            continue;
        }
        match byte {
            b'[' | b'(' => depth += 1,
            b']' | b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// One past the end of the item a gating attribute applies to, read from
/// `from`. A braced item runs to its matching `}` — continuing past `=>` when
/// the braces were a match-arm pattern — and a brace-less one to its `;` or
/// `,`. An unmatched `}` closes the enclosing block, so the item ends before it.
fn gated_item_end(code: &[u8], in_literal: &[bool], from: usize) -> usize {
    let n = code.len();
    let (mut parens, mut angles) = (0i32, 0i32);
    let mut k = from;
    while k < n {
        if in_literal[k] {
            k += 1;
            continue;
        }
        match code[k] {
            b'(' | b'[' => parens += 1,
            b')' | b']' => parens -= 1,
            b'<' => angles += 1,
            b'>' if k > 0 && matches!(code[k - 1], b'-' | b'=') => {}
            b'>' => angles = (angles - 1).max(0),
            b';' if parens <= 0 => return k + 1,
            b',' if parens <= 0 && angles == 0 => return k + 1,
            b'}' => return k,
            b'{' => {
                let close = matching_brace(code, in_literal, k);
                let mut after = close + 1;
                while after < n && code[after].is_ascii_whitespace() {
                    after += 1;
                }
                if code[after..].starts_with(b"=>") {
                    k = after + 2;
                    continue;
                }
                if after < n && matches!(code[after], b';' | b',') {
                    return after + 1;
                }
                return close + 1;
            }
            _ => {}
        }
        k += 1;
    }
    n
}

/// Blank every cfg(test)-gated item, so the view holds production code only.
/// Attribute text inside a string literal is not an attribute.
fn mask_test_gated_items(code: &mut [u8], literals: &[StrLit]) {
    let in_literal = literal_mask(code.len(), literals);
    let mut i = 0usize;
    while i < code.len() {
        if in_literal[i] || code[i] != b'#' || code.get(i + 1) != Some(&b'[') {
            i += 1;
            continue;
        }
        let Some(close) = attribute_end(code, &in_literal, i) else {
            break;
        };
        let attr = String::from_utf8_lossy(&code[i..=close]).into_owned();
        if !attr_gates_test_code(&attr) {
            i = close + 1;
            continue;
        }
        let end = gated_item_end(code, &in_literal, close + 1);
        blank(code, i, end);
        i = end;
    }
}

/// The fn name on a (trimmed) signature line, after any `pub`/`pub(…)`/
/// `const`/`async`/`unsafe` modifiers.
fn parse_fn_name(trimmed: &str) -> Option<String> {
    let mut rest = trimmed;
    loop {
        if let Some(r) = rest.strip_prefix("pub(") {
            rest = r[r.find(')')? + 1..].trim_start();
            continue;
        }
        let Some(r) = ["pub ", "const ", "async ", "unsafe "]
            .iter()
            .find_map(|m| rest.strip_prefix(m))
        else {
            break;
        };
        rest = r.trim_start();
    }
    let rest = rest.strip_prefix("fn ")?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_owned())
}

/// The enclosing fn of every skeleton line (index 0 is line 1). The scope ends
/// when the brace depth returns to the signature's level after entering the
/// body, so an exemption cannot leak onto a later fn whose `fn` keyword and
/// name are split across lines.
fn enclosing_fns(skeleton: &[String]) -> Vec<Option<String>> {
    let mut scopes = Vec::with_capacity(skeleton.len());
    let mut current: Option<String> = None;
    let (mut depth, mut fn_depth, mut in_body) = (0usize, 0usize, false);
    for line in skeleton {
        let trimmed = line.trim();
        if let Some(name) = parse_fn_name(trimmed) {
            current = Some(name);
            fn_depth = depth;
            in_body = false;
        }
        scopes.push(current.clone());
        for byte in line.bytes() {
            match byte {
                b'{' => {
                    depth += 1;
                    in_body |= current.is_some() && depth > fn_depth;
                }
                b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        let bodiless = !in_body && depth == fn_depth && trimmed.ends_with(';');
        if (in_body && depth <= fn_depth) || bodiless {
            current = None;
            in_body = false;
        }
    }
    scopes
}

/// Skeleton statements: consecutive lines joined until one whose trimmed text
/// ends in `{`, `}`, `;` or `,`. Each is `(first line, whitespace-normalised
/// text)`; a statement starts at its first non-blank line.
fn statement_windows(skeleton: &[String]) -> Vec<(usize, String)> {
    let mut windows = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (idx, line) in skeleton.iter().enumerate() {
        let trimmed = line.trim();
        if current.is_none() && trimmed.is_empty() {
            continue;
        }
        let (_, words) = current.get_or_insert_with(|| (idx + 1, Vec::new()));
        words.extend(trimmed.split_whitespace());
        if trimmed.ends_with(['{', '}', ';', ',']) {
            let (first, words) = current.take().expect("window just inserted");
            windows.push((first, words.join(" ")));
        }
    }
    windows.extend(current.map(|(first, words)| (first, words.join(" "))));
    windows
}

fn basename(label: &str) -> &str {
    Path::new(label)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(label)
}

#[derive(Debug)]
struct Finding {
    file: String,
    line: usize,
    text: String,
}

fn render(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("  {}:{}: {}", f.file, f.line, f.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
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

/// `(crate-relative label, contents)` of every `src/**/*.rs`, sorted.
fn crate_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let src =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {label}: {e}"));
            (label, src)
        })
        .collect()
}

/// The rule every row applies: live never exceeds the committed baseline.
fn within_baseline(live: usize, baseline: usize) -> bool {
    live <= baseline
}

// ── row (i): open-coded finite-positive checks ──────────────────────────────

const ROW_I_BASELINE: usize = 0;

/// The one (file basename, fn) allowed to spell the predicate.
const ROW_I_PREDICATE_HOME: (&str, &str) = ("op_args.rs", "is_positive_finite");

#[derive(Debug, Default)]
struct RowI {
    live: Vec<Finding>,
    exempt: Vec<Finding>,
}

/// An `is_finite` call together with a strict-positivity comparison against a
/// float zero, in either operand order and any literal spelling (`0.0`, `0.`,
/// `0f64`, `0.0_f64`): `r > 0`, `0 < r`, or the negated `r <= 0`, `0 >= r`.
/// Non-negativity (`r >= 0.0`, `0.0 <= r`) is not this row.
///
/// The row reads one statement at a time, so a check split across two
/// statements, or spelt through another predicate (`is_sign_positive`,
/// `partial_cmp`), is not seen: it holds the open-coded comparison at its
/// baseline, it does not ban every way of writing the predicate.
fn is_open_coded_finite_positive(window: &str) -> bool {
    has_ident(window, "is_finite") && compares_strictly_with_float_zero(window)
}

fn has_ident(text: &str, ident: &str) -> bool {
    ident_offsets(text, ident).next().is_some()
}

/// Byte offsets where `ident` occurs as a whole token.
fn ident_offsets<'t>(text: &'t str, ident: &'t str) -> impl Iterator<Item = usize> + 't {
    let bytes = text.as_bytes();
    text.match_indices(ident).filter_map(move |(at, _)| {
        let end = at + ident.len();
        let whole = starts_token(bytes, at) && bytes.get(end).is_none_or(|&b| !is_ident_byte(b));
        whole.then_some(at)
    })
}

/// One past the end of the float literal starting at `at` when it is a zero
/// (`0.0`, `0.`, `0f64`, `0.0_f64`), or `None`. An integer `0`, a range `0..`,
/// a method call `0.max(…)` and a non-zero value are not.
fn float_zero_literal_end(bytes: &[u8], at: usize) -> Option<usize> {
    let digits_end = |from: usize| {
        from + bytes[from..]
            .iter()
            .take_while(|&&b| b.is_ascii_digit() || b == b'_')
            .count()
    };
    let mut end = digits_end(at);
    let has_fraction = bytes.get(end) == Some(&b'.')
        && bytes
            .get(end + 1)
            .is_none_or(|&b| !(b == b'.' || b == b'_' || b.is_ascii_alphabetic()));
    if has_fraction {
        end = digits_end(end + 1);
    }
    let is_zero = bytes[at..end]
        .iter()
        .all(|&b| matches!(b, b'0' | b'_' | b'.'));
    let has_suffix = bytes[end..].starts_with(b"f64") || bytes[end..].starts_with(b"f32");
    if has_suffix {
        end += 3;
    }
    let token_ends = bytes.get(end).is_none_or(|&b| !is_ident_byte(b));
    ((has_fraction || has_suffix) && is_zero && token_ends).then_some(end)
}

/// `r > 0`, `0 < r`, `r <= 0` or `0 >= r` for some float zero, read with
/// whitespace removed. `=>` and `->` are not comparisons.
fn compares_strictly_with_float_zero(window: &str) -> bool {
    let compact: Vec<u8> = window
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let zero_literals = (0..compact.len())
        .filter(|&at| compact[at] == b'0' && starts_token(&compact, at))
        .filter(|&at| at == 0 || compact[at - 1] != b'.')
        .filter_map(|at| float_zero_literal_end(&compact, at).map(|end| (at, end)));
    for (at, end) in zero_literals {
        let (before, after) = (&compact[..at], &compact[end..]);
        let zero_on_right = (before.ends_with(b">")
            && !before.ends_with(b"=>")
            && !before.ends_with(b"->")
            && !before.ends_with(b">>"))
            || before.ends_with(b"<=");
        let zero_on_left =
            (after.starts_with(b"<") && !after.starts_with(b"<=") && !after.starts_with(b"<<"))
                || after.starts_with(b">=");
        if zero_on_right || zero_on_left {
            return true;
        }
    }
    false
}

fn row_i_scan(label: &str, src: &str) -> RowI {
    let skeleton = production_view(src).skeleton_lines();
    let scopes = enclosing_fns(&skeleton);
    let source_lines: Vec<&str> = src.lines().collect();
    let base = basename(label);
    let mut row = RowI::default();
    for (first, window) in statement_windows(&skeleton) {
        if !is_open_coded_finite_positive(&window) {
            continue;
        }
        let finding = Finding {
            file: label.to_owned(),
            line: first,
            text: source_lines[first - 1].trim().to_owned(),
        };
        let at_home = scopes[first - 1]
            .as_deref()
            .is_some_and(|f| (base, f) == ROW_I_PREDICATE_HOME);
        if at_home {
            row.exempt.push(finding);
        } else {
            row.live.push(finding);
        }
    }
    row
}

#[test]
fn row_i_open_coded_finite_positive_checks_within_baseline() {
    let mut row = RowI::default();
    for (label, src) in crate_sources() {
        let file_row = row_i_scan(&label, &src);
        row.live.extend(file_row.live);
        row.exempt.extend(file_row.exempt);
    }
    assert!(
        within_baseline(row.live.len(), ROW_I_BASELINE),
        "row (i): {} open-coded finite-positive check(s) > baseline {ROW_I_BASELINE}. Route \
         through op_args::validate_positive_finite / validate_all_positive_finite; a new \
         instance needs a visible baseline edit:\n{}",
        row.live.len(),
        render(&row.live)
    );
    assert!(
        !row.exempt.is_empty(),
        "row (i): the predicate's one home {}::{} is no longer seen, so the row may be \
         matching nothing; update ROW_I_PREDICATE_HOME in the same diff that moves it",
        ROW_I_PREDICATE_HOME.0,
        ROW_I_PREDICATE_HOME.1
    );
}

fn unit_test_attributes(view: &ProductionView) -> usize {
    view.skeleton_lines()
        .iter()
        .map(|line| line.matches("#[test]").count())
        .sum()
}

/// `{` count minus `}` count, string-literal contents excluded.
fn brace_imbalance(view: &ProductionView) -> isize {
    view.skeleton_lines()
        .iter()
        .flat_map(|line| line.bytes())
        .map(|byte| match byte {
            b'{' => 1,
            b'}' => -1,
            _ => 0,
        })
        .sum()
}

/// The scanner over the real sources, by property rather than by name. A
/// comment, string or char literal read as code would unbalance the braces
/// the lexer sees, which moves where a masked item ends; a masked item that
/// ends early leaks a `#[test]` into the production view.
#[test]
fn production_views_of_the_crate_sources_are_balanced_and_free_of_unit_tests() {
    let sources = crate_sources();
    let lexed_unit_tests: usize = sources
        .iter()
        .map(|(_, src)| unit_test_attributes(&lexed_view(src)))
        .sum();
    assert!(
        lexed_unit_tests > 0,
        "src/ carries no #[test] attribute, so the leak check below reads nothing"
    );
    for (label, src) in &sources {
        assert_eq!(
            brace_imbalance(&lexed_view(src)),
            0,
            "{label}: the lexer reads unbalanced braces, so it mis-reads a comment, \
             string or char literal"
        );
        let view = production_view(src);
        assert_eq!(
            brace_imbalance(&view),
            0,
            "{label}: the production view is unbalanced, so a masked item ended \
             inside another block"
        );
        assert_eq!(
            unit_test_attributes(&view),
            0,
            "{label}: a #[test] survives the mask, so a cfg(test)-gated item ended early"
        );
    }
}

// ── row (ii): numeric decode in the handle ──────────────────────────────────

const ROW_II_BASELINE: usize = 0;

/// The actor shell the row watches: it forwards ops, it does not decode them.
const ROW_II_FILE: &str = "handle.rs";

/// One finding per `.as_f64()` in the production view of `handle.rs`.
fn row_ii_scan(label: &str, src: &str) -> Vec<Finding> {
    if basename(label) != ROW_II_FILE {
        return Vec::new();
    }
    let source_lines: Vec<&str> = src.lines().collect();
    let mut findings = Vec::new();
    for (idx, line) in production_view(src).skeleton_lines().iter().enumerate() {
        for _ in line.matches(".as_f64()") {
            findings.push(Finding {
                file: label.to_owned(),
                line: idx + 1,
                text: source_lines[idx].trim().to_owned(),
            });
        }
    }
    findings
}

#[test]
fn row_ii_handle_as_f64_within_baseline() {
    let sources = crate_sources();
    assert!(
        sources
            .iter()
            .any(|(label, _)| basename(label) == ROW_II_FILE),
        "row (ii): src/{ROW_II_FILE} is gone; move the row with it in the same diff"
    );
    let live: Vec<Finding> = sources
        .iter()
        .flat_map(|(label, src)| row_ii_scan(label, src))
        .collect();
    assert!(
        within_baseline(live.len(), ROW_II_BASELINE),
        "row (ii): {} `.as_f64()` decode(s) in {ROW_II_FILE} production code > baseline \
         {ROW_II_BASELINE}. Decode in OcctKernel via op_args::extract_length_f64; a new \
         instance needs a visible baseline edit:\n{}",
        live.len(),
        render(&live)
    );
}

// ── homed uses: rows (iv)–(vi) ──────────────────────────────────────────────

/// A row whose identifiers may be used only inside their home fns. A use is a
/// whole-token occurrence in the production view that does not name a `fn`
/// declaration, so a call and a bare path (a fn-pointer table entry) both
/// count. Every home must be seen using one: a moved home or a reshaped FFI
/// reds the row instead of leaving it matching nothing.
struct HomedRow {
    id: &'static str,
    idents: &'static [&'static str],
    /// When set, only a use immediately preceded by this text counts.
    receiver: Option<&'static str>,
    /// `(file basename, fn)` pairs where a use is at home.
    homes: &'static [(&'static str, &'static str)],
    baseline: usize,
    remedy: &'static str,
}

#[derive(Debug, Default)]
struct HomedScan {
    live: Vec<Finding>,
    homes_seen: Vec<(String, String)>,
}

/// Is the token at `at` the name a `fn` keyword declares?
fn is_declared_name(text: &str, at: usize) -> bool {
    let before = text[..at].trim_end();
    before
        .strip_suffix("fn")
        .is_some_and(|rest| starts_token(before.as_bytes(), rest.len()))
}

fn has_receiver(text: &str, at: usize, receiver: &str) -> bool {
    text[..at]
        .strip_suffix(receiver)
        .is_some_and(|rest| starts_token(text.as_bytes(), rest.len()))
}

/// Byte offsets of every use of the row's identifiers in `text`, in order.
fn use_offsets(row: &HomedRow, text: &str) -> Vec<usize> {
    let mut offsets: Vec<usize> = row
        .idents
        .iter()
        .flat_map(|ident| ident_offsets(text, ident))
        .filter(|&at| !is_declared_name(text, at))
        .filter(|&at| row.receiver.is_none_or(|r| has_receiver(text, at, r)))
        .collect();
    offsets.sort_unstable();
    offsets
}

fn homed_scan(row: &HomedRow, label: &str, src: &str) -> HomedScan {
    let skeleton = production_view(src).skeleton_lines();
    let scopes = enclosing_fns(&skeleton);
    let text = skeleton.join("\n");
    let source_lines: Vec<&str> = src.lines().collect();
    let base = basename(label);
    let mut scan = HomedScan::default();
    for at in use_offsets(row, &text) {
        let line = text[..at].matches('\n').count() + 1;
        let home = scopes[line - 1]
            .as_deref()
            .filter(|&f| row.homes.contains(&(base, f)));
        match home {
            Some(f) => {
                let seen = (base.to_owned(), f.to_owned());
                if !scan.homes_seen.contains(&seen) {
                    scan.homes_seen.push(seen);
                }
            }
            None => scan.live.push(Finding {
                file: label.to_owned(),
                line,
                text: source_lines[line - 1].trim().to_owned(),
            }),
        }
    }
    scan
}

/// The row's homes absent from `seen`, as `file::fn`.
fn unseen_homes(row: &HomedRow, seen: &[(String, String)]) -> Vec<String> {
    row.homes
        .iter()
        .filter(|&&(file, f)| {
            !seen
                .iter()
                .any(|(s_file, s_fn)| s_file == file && s_fn == f)
        })
        .map(|(file, f)| format!("{file}::{f}"))
        .collect()
}

fn assert_homed_row(row: &HomedRow) {
    let mut scan = HomedScan::default();
    for (label, src) in crate_sources() {
        let file_scan = homed_scan(row, &label, &src);
        scan.live.extend(file_scan.live);
        scan.homes_seen.extend(file_scan.homes_seen);
    }
    assert!(
        within_baseline(scan.live.len(), row.baseline),
        "row ({}): {} use(s) of {:?} outside their home(s) > baseline {}. Instead, {}; a new \
         instance needs a visible baseline edit:\n{}",
        row.id,
        scan.live.len(),
        row.idents,
        row.baseline,
        row.remedy,
        render(&scan.live)
    );
    let unseen = unseen_homes(row, &scan.homes_seen);
    assert!(
        unseen.is_empty(),
        "row ({}): home(s) {} no longer use any of {:?}, so the row may be matching nothing; \
         move or retire the row in the same diff that moves the home or reshapes the FFI",
        row.id,
        unseen.join(", "),
        row.idents
    );
}

// ── row (iv): sweep-history decode ──────────────────────────────────────────

const ROW_IV: HomedRow = HomedRow {
    id: "iv",
    idents: &["sweep_op_history_face_modified"],
    receiver: None,
    homes: &[("history_decode.rs", "decode_sweep_history")],
    baseline: 0,
    remedy: "decode through history_decode::decode_sweep_history",
};

#[test]
fn row_iv_sweep_history_decode_within_baseline() {
    assert_homed_row(&ROW_IV);
}

// ── row (vi): topology getters ──────────────────────────────────────────────

const ROW_VI: HomedRow = HomedRow {
    id: "vi",
    idents: &["get_edges", "get_faces", "get_vertices"],
    receiver: None,
    homes: &[("lib.rs", "extract_subshapes")],
    baseline: 0,
    remedy: "enumerate sub-shapes through OcctKernel::extract_subshapes",
};

#[test]
fn row_vi_topology_getters_only_in_extract_subshapes() {
    assert_homed_row(&ROW_VI);
}

// ── self-tests: scanner ─────────────────────────────────────────────────────

/// A synthetic source written as an indented raw string, minus its leading
/// newline, so line 1 is the snippet's first line.
fn snippet(s: &str) -> &str {
    s.strip_prefix('\n').unwrap_or(s)
}

fn live_lines(label: &str, src: &str) -> Vec<usize> {
    row_i_scan(label, src).live.iter().map(|f| f.line).collect()
}

fn code_text(src: &str) -> String {
    String::from_utf8_lossy(&production_view(src).code).into_owned()
}

#[test]
fn scanner_ignores_line_doc_and_nested_block_comments() {
    let src = snippet(
        r#"
// if !(a.is_finite() && a > 0.0) {
/// if !(b.is_finite() && b > 0.0) {
/* outer /* if !(c.is_finite() && c > 0.0) { */ if !(d.is_finite() && d > 0.0) { */
fn f(e: f64) {
    if !(e.is_finite() && e > 0.0) {
        return;
    }
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![5]);
}

#[test]
fn scanner_does_not_start_a_comment_at_slashes_inside_a_string() {
    let src = snippet(
        r#"
fn f(r: f64) {
    let _url = "https://example.com"; if !(r.is_finite() && r > 0.0) {
        return;
    }
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![2]);
    assert!(code_text(src).contains("\"https://example.com\"; if !(r.is_finite()"));
}

#[test]
fn scanner_keeps_continued_strings_with_braces_inside_their_literal() {
    let src = snippet(
        r#"
#[cfg(test)]
mod tests {
    fn t(x: f64) {
        let _s = "open { \
                  close } }";
        if !(x.is_finite() && x > 0.0) {}
    }
}
fn prod(r: f64) {
    let _s = "}} \
              {";
    if !(r.is_finite() && r > 0.0) {}
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![12]);
}

#[test]
fn scanner_char_literals_do_not_end_a_masked_mod_and_lifetimes_stay_code() {
    let src = snippet(
        r#"
#[cfg(test)]
mod tests {
    fn t(x: f64) {
        let _close = '}';
        let _open = '{';
        let _quote = '"';
        if !(x.is_finite() && x > 0.0) {}
    }
}
fn prod<'a>(s: &'a str, r: f64) -> &'a str {
    if !(r.is_finite() && r > 0.0) {}
    s
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![11]);
    assert!(code_text(src).contains("fn prod<'a>(s: &'a str, r: f64) -> &'a str {"));
}

#[test]
fn scanner_masks_test_gated_items() {
    let src = snippet(
        r#"
#[cfg(all(test, has_occt))]
#[allow(clippy::needless_range_loop)]
mod tests {
    fn t(x: f64) {
        if !(x.is_finite() && x > 0.0) {}
    }
}
#[cfg(test)]
use std::collections::HashMap;
#[cfg(feature = "test-fixtures")]
fn fixture(y: f64) {
    if !(y.is_finite() && y > 0.0) {}
}
fn prod(r: f64) {
    if !(r.is_finite() && r > 0.0) {}
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![15]);
    let code = code_text(src);
    assert!(
        !code.contains("HashMap"),
        "`#[cfg(test)] use …;` must be masked"
    );
    assert!(
        code.contains("fn prod(r: f64) {"),
        "masking `use …;` must stop at its `;`"
    );
}

#[test]
fn scanner_masks_a_test_gated_match_arm_and_enum_variant_only() {
    let src = snippet(
        r#"
enum Req {
    #[cfg(feature = "test-fixtures")]
    Fixture(f64),
    Real,
}
fn dispatch(req: Req, r: f64) {
    match req {
        #[cfg(feature = "test-fixtures")]
        Req::Fixture { x } => {
            if !(x.is_finite() && x > 0.0) {}
        }
        Req::Real => {
            if !(r.is_finite() && r > 0.0) {}
        }
    }
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![13]);
    let code = code_text(src);
    assert!(!code.contains("Fixture"));
    assert!(code.contains("    Real,"));
}

#[test]
fn scanner_keeps_production_cfg_items() {
    let src = snippet(
        r#"
#[cfg(not(test))]
fn f(a: f64) {
    if !(a.is_finite() && a > 0.0) {}
}
#[cfg(has_occt)]
fn g(b: f64) {
    if !(b.is_finite() && b > 0.0) {}
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![3, 7]);
}

#[test]
fn scanner_reads_attribute_text_inside_a_string_as_a_string() {
    // The literal sits in a tail expression: were it read as an attribute, the
    // item walk would run past this fn's `}` and swallow `prod`'s body.
    let src = snippet(
        r##"
fn is_test_attr(s: &str) -> bool {
    s.starts_with("#[cfg(test)]")
}
fn prod(r: f64) {
    if !(r.is_finite() && r > 0.0) {}
}
"##,
    );
    assert_eq!(live_lines("lib.rs", src), vec![5]);
    assert!(code_text(src).contains("s.starts_with(\"#[cfg(test)]\")"));
}

// ── self-tests: row (i) ─────────────────────────────────────────────────────

#[test]
fn row_i_flags_an_open_coded_check_and_clears_once_routed() {
    let open_coded = snippet(
        r#"
fn cylinder(r: f64) -> Result<(), E> {
    if !(r.is_finite() && r > 0.0) {
        return Err(e());
    }
    Ok(())
}
"#,
    );
    assert_eq!(live_lines("lib.rs", open_coded), vec![2]);

    let routed = snippet(
        r#"
fn cylinder(r: f64) -> Result<(), E> {
    validate_positive_finite(r, "x")?;
    Ok(())
}
"#,
    );
    assert_eq!(live_lines("lib.rs", routed), Vec::<usize>::new());
}

#[test]
fn row_i_reports_a_multi_line_condition_once_at_its_first_line() {
    let src = snippet(
        r#"
fn make_box(w: f64, h: f64) -> Result<(), E> {
    if !(w.is_finite()
        && w > 0.0
        && h.is_finite()
        && h > 0.0)
    {
        return Err(e());
    }
    Ok(())
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), vec![2]);
}

#[test]
fn row_i_flags_the_non_positive_spelling_but_not_non_negativity() {
    let non_positive = snippet(
        r#"
fn arc(radius: &f64) {
    if !radius.is_finite() || *radius <= 0.0 {
        return;
    }
}
"#,
    );
    assert_eq!(live_lines("lib.rs", non_positive), vec![2]);

    let non_negative = snippet(
        r#"
fn cone(x: f64) {
    if !(x.is_finite() && x >= 0.0) {
        return;
    }
}
"#,
    );
    assert_eq!(live_lines("lib.rs", non_negative), Vec::<usize>::new());
}

/// `expr` as the one statement of a fn body, on line 2.
fn statement_snippet(expr: &str) -> String {
    format!("fn f(r: f64, n: usize, k: usize) {{\n    let _ = {expr};\n}}\n")
}

#[test]
fn row_i_flags_every_zero_spelling_in_either_operand_order() {
    for expr in [
        "r.is_finite() && 0.0 < r",
        "r.is_finite() && r > 0f64",
        "r.is_finite() && r > 0.",
        "r.is_finite() && r>0.0_f64",
        "!r.is_finite() || 0.0 >= r",
        "f64::is_finite(r) && r > 0.0",
    ] {
        assert_eq!(
            live_lines("lib.rs", &statement_snippet(expr)),
            vec![2],
            "{expr}"
        );
    }
}

#[test]
fn row_i_ignores_non_negativity_non_zero_bounds_and_non_comparisons() {
    for expr in [
        "r.is_finite() && 0.0 <= r",
        "r.is_finite() && r > 0.5",
        "r.is_finite() && r > 10.0",
        "r.is_finite() && n > 0",
        "r.is_finite() && (0..n).contains(&k)",
        "r.is_finite() && 0.max(n) < k",
        "match r.is_finite() { true => 0.0, false => 1.0 }",
        "r.is_finite_and_positive() && r > 0.0",
    ] {
        assert_eq!(
            live_lines("lib.rs", &statement_snippet(expr)),
            Vec::<usize>::new(),
            "{expr}"
        );
    }
}

#[test]
fn row_i_exempts_only_the_predicate_home_in_op_args_rs() {
    let src = snippet(
        r#"
fn is_positive_finite(v: f64) -> bool {
    v.is_finite() && v > 0.0
}
"#,
    );
    let in_op_args = row_i_scan("src/op_args.rs", src);
    assert_eq!((in_op_args.live.len(), in_op_args.exempt.len()), (0, 1));
    let in_lib = row_i_scan("src/lib.rs", src);
    assert_eq!((in_lib.live.len(), in_lib.exempt.len()), (1, 0));
}

#[test]
fn row_i_exemption_does_not_leak_past_the_home_body() {
    let tail = r#"
pub(crate) fn
    sneaky(r: f64) -> bool {
    if !(r.is_finite() && r > 0.0) {
        return false;
    }
    true
}
"#;
    for home in [
        "fn is_positive_finite(v: f64) -> bool {\n    v.is_finite() && v > 0.0\n}\n",
        "fn is_positive_finite(v: f64) -> bool { v.is_finite() && v > 0.0 }\n",
    ] {
        let src = format!("{home}{tail}");
        let row = row_i_scan("op_args.rs", &src);
        assert_eq!(
            (row.live.len(), row.exempt.len()),
            (1, 1),
            "home spelled {home:?}: {row:?}"
        );
        assert!(row.live[0].text.contains("r.is_finite()"), "{row:?}");
    }
}

#[test]
fn row_i_ignores_the_construct_inside_a_test_mod() {
    let src = snippet(
        r#"
#[cfg(test)]
mod tests {
    fn t(r: f64) {
        if !(r.is_finite() && r > 0.0) {
            return;
        }
    }
}
"#,
    );
    assert_eq!(live_lines("lib.rs", src), Vec::<usize>::new());
}

// ── self-tests: row (ii) ────────────────────────────────────────────────────

fn row_ii_count(label: &str, src: &str) -> usize {
    row_ii_scan(label, src).len()
}

#[test]
fn row_ii_flags_a_handle_decode_and_clears_once_forwarded() {
    let decoding = snippet(
        r#"
fn serve(kernel: &mut OcctKernel, radius: &Value) {
    match radius.as_f64() {
        Some(r) => kernel.fillet(r),
        None => fail(),
    }
}
"#,
    );
    assert_eq!(row_ii_count("src/handle.rs", decoding), 1);

    let forwarding = snippet(
        r#"
fn serve(kernel: &mut OcctKernel, op: GeometryOp, reply: Sender<R>) {
    let _ = reply.send(kernel.execute_with_history(&op));
}
"#,
    );
    assert_eq!(row_ii_count("src/handle.rs", forwarding), 0);
}

#[test]
fn row_ii_ignores_comments_strings_and_test_code() {
    let src = snippet(
        r#"
/// Never call `.as_f64()` here.
fn serve() {
    // radius.as_f64() was the old decode
    let _hint = "use .as_f64() in the kernel";
}
#[cfg(all(test, has_occt))]
mod tests {
    fn t(v: &Value) -> Option<f64> {
        v.as_f64()
    }
}
"#,
    );
    assert_eq!(row_ii_count("src/handle.rs", src), 0);
}

#[test]
fn row_ii_is_scoped_to_handle_rs() {
    let src = "fn decode(v: &Value) -> Option<f64> {\n    v.as_f64()\n}\n";
    assert_eq!(row_ii_count("src/handle.rs", src), 1);
    assert_eq!(row_ii_count("src/lib.rs", src), 0);
}

// ── self-tests: homed uses and row (iv) ─────────────────────────────────────

fn homed_live_lines(row: &HomedRow, label: &str, src: &str) -> Vec<usize> {
    homed_scan(row, label, src)
        .live
        .iter()
        .map(|f| f.line)
        .collect()
}

#[test]
fn row_iv_flags_a_decode_outside_its_home_and_clears_at_home() {
    let copied = snippet(
        r#"
fn extrude_with_history(&mut self) -> R {
    let face_modified =
        decode_history_records(ffi::ffi::sweep_op_history_face_modified(&history));
}
"#,
    );
    let scan = homed_scan(&ROW_IV, "src/lib.rs", copied);
    assert_eq!(
        scan.live.iter().map(|f| f.line).collect::<Vec<_>>(),
        vec![3]
    );
    assert!(scan.homes_seen.is_empty(), "{scan:?}");

    let homed = snippet(
        r#"
pub(crate) fn decode_sweep_history(
    mut history: cxx::UniquePtr<ffi::SweepOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, SweepOpHistoryRecords) {
    let face_modified = decode_history_records(ffi::sweep_op_history_face_modified(&history));
}
"#,
    );
    let scan = homed_scan(&ROW_IV, "src/history_decode.rs", homed);
    assert!(scan.live.is_empty(), "{scan:?}");
    assert_eq!(
        unseen_homes(&ROW_IV, &scan.homes_seen),
        Vec::<String>::new()
    );
}

#[test]
fn row_iv_does_not_count_the_ffi_declaration() {
    let src = snippet(
        r#"
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        fn sweep_op_history_face_modified(history: &SweepOpHistory) -> Vec<u32>;
        pub(crate) fn
            sweep_op_history_face_modified(history: &SweepOpHistory) -> Vec<u32>;
    }
}
"#,
    );
    assert_eq!(
        homed_live_lines(&ROW_IV, "src/ffi.rs", src),
        Vec::<usize>::new()
    );
}

#[test]
fn row_iv_counts_a_fn_pointer_path_even_in_the_home_file() {
    let src = snippet(
        r#"
const SWEEP_OP_ACCESSORS: Accessors<ffi::SweepOpHistory> = Accessors {
    face_modified: ffi::sweep_op_history_face_modified,
};
"#,
    );
    assert_eq!(
        homed_live_lines(&ROW_IV, "src/history_decode.rs", src),
        vec![2]
    );
}

#[test]
fn row_iv_ignores_comments_strings_and_test_code() {
    let src = snippet(
        r#"
/// Reads `sweep_op_history_face_modified` first.
fn extrude_with_history() {
    // ffi::ffi::sweep_op_history_face_modified(&history)
    let _hint = "sweep_op_history_face_modified";
}
#[cfg(all(test, has_occt))]
mod tests {
    fn t(h: &SweepOpHistory) -> Vec<u32> {
        ffi::ffi::sweep_op_history_face_modified(h)
    }
}
"#,
    );
    assert_eq!(
        homed_live_lines(&ROW_IV, "src/lib.rs", src),
        Vec::<usize>::new()
    );
}

#[test]
fn row_iv_counts_whole_tokens_only() {
    let src = snippet(
        r#"
fn extrude_with_history() {
    let _n = ffi::ffi::sweep_op_history_face_modified_count(&history);
    let _m = my_sweep_op_history_face_modified(&history);
}
"#,
    );
    assert_eq!(
        homed_live_lines(&ROW_IV, "src/lib.rs", src),
        Vec::<usize>::new()
    );
}

#[test]
fn row_iv_home_is_the_fn_in_history_decode_rs_only() {
    let src = snippet(
        r#"
fn decode_sweep_history(history: UniquePtr<SweepOpHistory>) -> R {
    decode_history_records(ffi::ffi::sweep_op_history_face_modified(&history))
}
"#,
    );
    let scan = homed_scan(&ROW_IV, "src/lib.rs", src);
    assert_eq!(scan.live.len(), 1, "{scan:?}");
    assert_eq!(
        unseen_homes(&ROW_IV, &scan.homes_seen),
        vec!["history_decode.rs::decode_sweep_history".to_owned()]
    );
}

#[test]
fn row_iv_reports_a_home_that_uses_none_of_its_idents_as_unseen() {
    let src = snippet(
        r#"
pub(crate) fn decode_sweep_history(history: UniquePtr<SweepOpHistory>) -> R {
    decode_with(&SWEEP_OP_ACCESSORS, history)
}
"#,
    );
    let scan = homed_scan(&ROW_IV, "src/history_decode.rs", src);
    assert!(scan.live.is_empty(), "{scan:?}");
    assert_eq!(
        unseen_homes(&ROW_IV, &scan.homes_seen),
        vec!["history_decode.rs::decode_sweep_history".to_owned()]
    );
}

// ── self-tests: row (vi) ────────────────────────────────────────────────────

#[test]
fn row_vi_flags_three_extract_bodies_and_clears_behind_extract_subshapes() {
    let three_bodies = snippet(
        r#"
fn get_edges(shape: &OcctShape) -> Result<UniquePtr<OcctShapeVec>>;
pub fn extract_edges(&mut self, handle: GeometryHandleId) -> R {
    let vec = ffi::ffi::get_edges(shape).map_err(q)?;
}
pub fn extract_faces(&mut self, handle: GeometryHandleId) -> R {
    let vec = ffi::ffi::get_faces(shape).map_err(q)?;
}
pub fn extract_vertices(&mut self, handle: GeometryHandleId) -> R {
    let vec = ffi::ffi::get_vertices(shape)
        .map_err(q)?;
}
"#,
    );
    let scan = homed_scan(&ROW_VI, "src/lib.rs", three_bodies);
    assert_eq!(
        scan.live.iter().map(|f| f.line).collect::<Vec<_>>(),
        vec![3, 6, 9]
    );
    assert_eq!(
        unseen_homes(&ROW_VI, &scan.homes_seen),
        vec!["lib.rs::extract_subshapes".to_owned()]
    );

    let one_body = snippet(
        r#"
fn extract_subshapes(
    &mut self,
    parent: GeometryHandleId,
    kind: SubShapeKind,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let vec = match kind {
        SubShapeKind::Edge => ffi::ffi::get_edges(shape),
        SubShapeKind::Face => ffi::ffi::get_faces(shape),
        SubShapeKind::Vertex => ffi::ffi::get_vertices(shape),
    }
    .map_err(q)?;
}
pub fn extract_edges(&mut self, handle: GeometryHandleId) -> R {
    self.extract_subshapes(handle, SubShapeKind::Edge)
}
pub fn extract_faces(&mut self, handle: GeometryHandleId) -> R {
    self.extract_subshapes(handle, SubShapeKind::Face)
}
pub fn extract_vertices(&mut self, handle: GeometryHandleId) -> R {
    self.extract_subshapes(handle, SubShapeKind::Vertex)
}
"#,
    );
    let scan = homed_scan(&ROW_VI, "src/lib.rs", one_body);
    assert!(scan.live.is_empty(), "{scan:?}");
    assert_eq!(
        unseen_homes(&ROW_VI, &scan.homes_seen),
        Vec::<String>::new()
    );
}
