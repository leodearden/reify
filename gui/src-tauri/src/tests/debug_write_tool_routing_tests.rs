//! INV-GUI-2 (`docs/invariants.md`) structural gate for the AI/MCP entry
//! point: every `reify_*` write tool reaches the delta baseline through one
//! of the two shared `*_and_refresh_baseline` seams, and none emits state
//! alongside it.
//!
//! The emit half spans a GRAMMAR, not the whole property, so read its reach
//! literally: a handler — or a helper it delegates to, one hop away — that
//! names any of [`PRIVATE_EMIT_IDENTIFIERS`] or makes a `.emit(` call. Two
//! residuals follow, both stated rather than papered over. An emit further
//! than one hop is out of reach of any depth-capped scan and is governed by
//! point (b) on `write_on_engine_and_refresh_baseline` ("Do NOT add a second
//! emit path here or in any caller") rather than by this gate. And an
//! emission shape named by none of those tokens is invisible, which is how
//! the half went vacuous once already — so its real-file assertion carries a
//! spliced POSITIVE CONTROL
//! ([`the_private_emit_sweep_fires_on_the_real_file_when_mutated`]) rather
//! than resting on an empty result meaning what it appears to.
//!
//! The claim is stated in prose — and NOT restated here — at point (a) on
//! `write_on_engine_and_refresh_baseline` in `gui/src-tauri/src/debug_server.rs`
//! and in `docs/debug-mcp-contract.md`'s "Two seams, ONE stated exception".
//! Note its shape: "one of the TWO shared seams", not "all five route through
//! `write_on_engine_and_refresh_baseline`". Citations here are SYMBOL names,
//! never line numbers, and `every_anchor_this_module_cites_still_exists`
//! checks them: a gate that exists to resist drift cannot itself ship
//! pointers that go stale on the next edit above them.
//!
//! This reads `debug_server.rs` as TEXT and links nothing, so unlike
//! `debug_boundary_tests` it carries NO `#[cfg(feature = "gui")]` gate. The
//! benefit that holds on EVERY scope is LINTING: `verify.sh` runs
//! `cargo clippy --workspace --all-targets -- -D warnings` without
//! `--features gui`, so gui-gated code is type-checked but never
//! lint-checked — the open gap owned by task #5841 — while an ungated module
//! is linted like the rest of the workspace today. Being ungated also costs
//! no `scripts/ensure-gui-sidecar-placeholder.sh` and no tauri/webkit2gtk/
//! OCCT link, so this gate runs under a plain `cargo test -p reify-gui --lib`
//! in any warm lane.
//!
//! NOT claimed: that a gui-gated test would be skipped by the merge gate.
//! `verify.sh` emits its `-p reify-gui --features gui` test pass whenever
//! `closure_reaches_reify_gui` holds, and that returns true unconditionally
//! for `--scope all` BY CONTRACT — so a gui-gated test runs there too. What
//! being ungated buys is the NARROW scopes, where that pass is not emitted:
//! the per-commit hook's `--scope staged` on a diff whose affected-crate
//! closure misses reify-gui — exactly the run during which someone adds a
//! bypassing tool elsewhere. Spelled out because the converse reading,
//! "reify-gui's gui-gated tests do not really run in the gate", is a
//! near-miss this repo has had to re-refute more than once.
//!
//! REDUNDANT ENUMERATION: the write-tool set is read from TWO independent
//! textual shapes — the `"reify_*" =>` dispatch arms and the `ToolDef`
//! registry advertising the same names — which must agree, so an arm the
//! scanner cannot read reds as a set DIFFERENCE rather than vanishing from
//! the sweep, whatever made it unreadable. Residual, stated rather than
//! papered over: a drop stays silent if BOTH enumerations miss the SAME tool,
//! and the registry-side non-vacuity floor is what catches them reaching zero
//! together.
//!
//! POSTURE: default-ASSERT, `REIFY_INV_GUI_2_BYPASS=1` as break-glass — same
//! convention as
//! `crates/reify-eval/tests/harness_cache/snapshot_cache_divergence_gate.rs`.
//! The warn-mode corpus sweep the task called for is re-performed
//! mechanically on every run, so a warn-only default would emit no signal on
//! a green tree and defer the leaf indefinitely. No `…_ENFORCE` alias:
//! against a default-assert state it would be a no-op.
//!
//! The synthetic corpus every negative and positive claim below is pinned
//! against lives in the sibling `debug_write_tool_routing_fixtures`; that
//! module's header says why it is split out.

use super::debug_write_tool_routing_fixtures::{
    ADVERTISED_BUT_UNDISPATCHED_SOURCE, ARM_SHAPES_SOURCE, BYPASSING_SOURCE,
    COMMENT_ONLY_MENTION_SOURCE, COMPLIANT_SOURCE, DELEGATED_PRIVATE_EMIT_SOURCE,
    EVENT_BUS_PRIVATE_EMIT_SOURCE, INLINE_ARM_SOURCE, LYING_SEAM_SOURCE, PRIVATE_EMIT_SOURCE,
    STRING_ONLY_MENTION_SOURCE,
};

/// One way a `reify_*` write tool can break INV-GUI-2.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum BypassKind {
    /// The handler never reaches a `*_and_refresh_baseline` seam, so the
    /// delta baseline silently goes stale after its write.
    NoBaselineRefresh,
    /// The handler — or a helper it delegates to, one hop away — emits state
    /// on its own alongside the shared seam: the second emission path point
    /// (b) on `write_on_engine_and_refresh_baseline` forbids.
    PrivateEmit,
    /// The arm's handler does not resolve to a top-level fn in this file — an
    /// inline arm, or a call shape the scan reads wrongly. Reported distinctly
    /// because "the checker never found the handler" and "the handler skips
    /// the seam" warrant different fixes; conflating them would have the
    /// report state a confident falsehood about the handler.
    UnresolvedHandler,
}

/// A single INV-GUI-2 violation, reported structurally so callers assert on
/// values rather than on a rendered message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Bypass {
    tool: String,
    handler: String,
    kind: BypassKind,
}

/// Parses the `"reify_<tool>" => <handler>` dispatch arms out of `code`,
/// returning `(tool, handler)` in source order — one entry per tool, so an
/// or-pattern yields N entries against its one shared handler. `code` must
/// already be comment-stripped.
///
/// ENUMERATED rather than hardcoded, with deliberately no read-only exemption
/// set, so a sixth `reify_*` tool must route or go red until someone
/// classifies it consciously — the gap INV-GUI-2 exists to close.
///
/// Once a `"reify_*"` pattern literal has been SEEN it is never dropped: an
/// unresolvable handler still yields its tool, which [`write_tool_bypasses`]
/// reports as [`BypassKind::UnresolvedHandler`]. Dropping was this checker's
/// one false-GREEN direction — a tool missing from this list is swept by
/// NEITHER half of the gate.
///
/// Retained approximation, in the module's fail-closed direction: the handler
/// is the FIRST `identifier(` call in the arm, so a block arm that calls
/// something else first (a guard, a `Box::pin`) resolves to the wrong fn and
/// false-POSITIVES. Red, never silently green.
fn dispatch_arms(code: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = code.lines().collect();
    let mut arms: Vec<(String, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some((tools, after_arrow)) = arm_pattern(line.trim()) else {
            continue;
        };
        if tools.is_empty() {
            continue;
        }
        let handler = arm_handler(after_arrow, &lines[i + 1..]).unwrap_or_default();
        arms.extend(tools.into_iter().map(|tool| (tool, handler.clone())));
    }
    arms
}

/// Splits a trimmed match-arm line into its `reify_*` pattern alternatives and
/// the text after the `=>`.
///
/// Alternatives are collected across `|` separators and filtered to the
/// `reify_` prefix, so `"open_file" | "reify_open_file" =>` still surfaces the
/// write tool. Returns `None` for any line that is not a string-literal arm
/// pattern — including the `"key": value` lines inside the registry's `json!`
/// schemas, which stop at the `:` and never reach a `=>`.
fn arm_pattern(trimmed: &str) -> Option<(Vec<String>, &str)> {
    let mut rest = trimmed;
    let mut tools = Vec::new();
    loop {
        let (literal, tail) = rest.strip_prefix('"')?.split_once('"')?;
        if literal.starts_with("reify_") {
            tools.push(literal.to_string());
        }
        rest = tail.trim_start();
        match rest.strip_prefix('|') {
            Some(tail) => rest = tail.trim_start(),
            None => return rest.strip_prefix("=>").map(|body| (tools, body)),
        }
    }
}

/// Resolves the fn an arm delegates to, searching the `=>` remainder first and
/// then the following lines — which is what reads the wrapped (`=>` then a
/// newline) and block (`=> { … }`) shapes as well as the single-line one.
///
/// The forward scan is BOUNDED at the next arm's pattern, the `_ =>`
/// catch-all, or a closing brace, so a block arm containing no call can never
/// run on and mis-attribute the NEXT arm's handler to this tool.
fn arm_handler(after_arrow: &str, following: &[&str]) -> Option<String> {
    if let Some(handler) = first_call_identifier(after_arrow) {
        return Some(handler.to_string());
    }
    for line in following {
        let trimmed = line.trim();
        if trimmed.starts_with('"') || trimmed.starts_with("_ =>") || trimmed.starts_with('}') {
            return None;
        }
        if let Some(handler) = first_call_identifier(line) {
            return Some(handler.to_string());
        }
    }
    None
}

/// The first `identifier(` call in `text`, if any.
fn first_call_identifier(text: &str) -> Option<&str> {
    let mut ident_start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() || c == '_' {
            ident_start.get_or_insert(i);
        } else {
            if c == '('
                && let Some(start) = ident_start
            {
                return Some(&text[start..i]);
            }
            ident_start = None;
        }
    }
    None
}

/// Names every `reify_*` tool the `ToolDef` registry ADVERTISES, in source
/// order — the `name:` field of each entry `tool_defs` returns. `code` must
/// already be comment-stripped.
///
/// This is the gate's SECOND, independent enumeration of the same tool set,
/// read from a struct-literal field rather than from a match pattern. The
/// `name: "reify_` key cannot collide: every `"reify_*"` literal in the real
/// file is either a registry entry or a dispatch arm, and nothing else.
fn registry_tool_names(code: &str) -> Vec<String> {
    code.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("name:")?.trim_start();
            let (tool_suffix, _) = rest.strip_prefix("\"reify_")?.split_once('"')?;
            Some(format!("reify_{tool_suffix}"))
        })
        .collect()
}

/// Names the advertised `reify_*` tools the dispatch scan never saw, sorted —
/// the module header's REDUNDANT ENUMERATION, made concrete.
///
/// It also catches a shape no arm parser could reach at all: a
/// registry-advertised tool dispatched by a non-literal path, falling into
/// `dispatch_tool`'s `_ =>` frontend-delegation catch-all.
fn unenumerated_tools(source: &str) -> Vec<String> {
    let code = strip_comments(source);
    let dispatched: Vec<String> = dispatch_arms(&code)
        .into_iter()
        .map(|(tool, _)| tool)
        .collect();
    let mut missing: Vec<String> = registry_tool_names(&code)
        .into_iter()
        .filter(|tool| !dispatched.contains(tool))
        .collect();
    missing.sort();
    missing
}

/// Parses the fn name out of a (trimmed) line that looks like a fn signature,
/// stripping leading `pub`/`pub(...)`/`async`/`unsafe` modifiers first.
/// Returns `None` if the line isn't a fn signature. Modelled on
/// `crates/reify-eval/tests/version_id_discipline_gate.rs`'s `parse_fn_name`;
/// the name scan stops at the first non-identifier char, so a generic
/// signature (`fn write_on_engine_and_refresh_baseline<F>(`) parses too.
fn parse_fn_name(trimmed: &str) -> Option<&str> {
    let mut rest = trimmed;
    while !rest.starts_with("fn ") {
        rest = if let Some(r) = rest.strip_prefix("pub(") {
            r[r.find(')')? + 1..].trim_start()
        } else if let Some(r) = rest.strip_prefix("pub ") {
            r.trim_start()
        } else if let Some(r) = rest.strip_prefix("async ") {
            r.trim_start()
        } else if let Some(r) = rest.strip_prefix("unsafe ") {
            r.trim_start()
        } else {
            return None;
        };
    }
    let rest = rest["fn ".len()..].trim_start();
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    (end > 0).then(|| &rest[..end])
}

/// Returns the source slice of the TOP-LEVEL fn named `name`, from its
/// signature line through the closing brace.
fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let span = fn_span(source, name)?;
    Some(&source[span.0..span.1])
}

/// The `(start, end)` byte offsets of the TOP-LEVEL fn named `name` — what
/// [`fn_body`] slices, and what [`splice_into_fn_body`] edits inside.
///
/// The terminator is the next line that is exactly `}` in column 0. That is
/// sound for `debug_server.rs`, which indents every nested block, so no inner
/// brace can reach column 0; a top-level fn's own closing brace is the first
/// that does. A file violating that convention truncates the body early,
/// which loses seam matches and therefore false-POSITIVES — red, never
/// silently green.
fn fn_span(source: &str, name: &str) -> Option<(usize, usize)> {
    let mut start: Option<usize> = None;
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        match start {
            None => {
                if parse_fn_name(line.trim_end()) == Some(name) {
                    start = Some(offset);
                }
            }
            Some(begin) => {
                if line.trim_end() == "}" {
                    return Some((begin, offset + line.len()));
                }
            }
        }
        offset += line.len();
    }
    None
}

/// `source` with `stmt` spliced in as the first statement of the TOP-LEVEL fn
/// named `fn_name` — the mutation
/// [`the_private_emit_sweep_fires_on_the_real_file_when_mutated`] controls
/// with.
///
/// `None`, never a silently unmutated copy, when the fn does not resolve: the
/// caller unwraps, so a splice that failed to apply reds its positive control
/// instead of passing it vacuously. A mutation test whose mutation did not
/// happen is the same false GREEN one level up that the control exists to
/// retire.
///
/// The fn is located by [`fn_span`] — the same walk the gate itself runs on,
/// so the control cannot drift onto a different notion of "top-level fn" than
/// the checks it is the floor for. The insertion point is the first `{` in
/// that span, which opens the body: a multi-line signature puts it several
/// lines below the `fn` keyword.
fn splice_into_fn_body(source: &str, fn_name: &str, stmt: &str) -> Option<String> {
    let (start, end) = fn_span(source, fn_name)?;
    let open = start + source[start..end].find('{')?;
    let (head, tail) = source.split_at(open + 1);
    Some(format!("{head}\n{stmt}{tail}"))
}

/// True when `body` names any identifier ending in `_and_refresh_baseline`
/// other than `own_name` (a fn's own signature line is part of its body).
fn names_a_seam(body: &str, own_name: &str) -> bool {
    identifiers(body).any(|id| id.ends_with("_and_refresh_baseline") && id != own_name)
}

/// True when top-level fn `name`'s own body satisfies `holds`, or a resolvable
/// top-level callee's body does exactly ONE delegation hop away.
///
/// One hop is exactly what `reify_open_file` needs — `handle_reify_open_file`
/// delegates its entire body to `open_path_into_engine`, which is where both
/// the seam call and the frontend push actually live — and an uncapped walk
/// would be a call-graph analyzer, far more machinery than the claim warrants.
///
/// The cap's failure direction DIFFERS per predicate, and both are stated
/// rather than papered over. For the seam check it is fail-CLOSED: a seam
/// reached deeper than one hop false-POSITIVES, so a human looks. For the
/// private-emit check it is fail-OPEN — an emit further than one hop from the
/// handler is missed — so applying the walk there REDUCES that gap rather than
/// closing it. It is the hop that matters today; a deeper emit is out of reach
/// of any depth-capped scan, and `write_on_engine_and_refresh_baseline`'s
/// point (b) prose ("Do NOT add a second emit path here or in any caller") is
/// what covers it.
fn within_one_hop(code: &str, name: &str, holds: impl Fn(&str, &str) -> bool) -> bool {
    let Some(body) = fn_body(code, name) else {
        return false;
    };
    holds(body, name)
        || identifiers(body)
            .filter(|callee| *callee != name)
            .any(|callee| fn_body(code, callee).is_some_and(|hop| holds(hop, callee)))
}

/// True when the top-level fn `name` reaches a `*_and_refresh_baseline` seam.
fn reaches_a_seam(code: &str, name: &str) -> bool {
    within_one_hop(code, name, names_a_seam)
}

/// The emission surface a write-tool handler must not reach into: naming any
/// of these is a second emission path running alongside the shared seam.
///
/// One point of truth for the grammar, with each entry carrying its own
/// reachability against today's `debug_server.rs`, so a reader learns which
/// arms can actually fire from the tokens themselves. None of the three has
/// any textual occurrence in that file today, so the set is a widening of
/// what the gate catches and not of what it reds on.
const PRIVATE_EMIT_IDENTIFIERS: [&str; 3] = [
    // The library's sanctioned `pub fn` wrapper (`crate::event_bus`), whose
    // own header steers callers to it. LIVE, and the likeliest shape.
    "emit_typed",
    // The library-side delta-to-events conversion `emit_delta` is built on
    // (`for (name, payload) in delta_to_events(delta)`). LIVE, and it catches
    // a hand-rolled emit loop even when the emit call itself sits past the
    // one-hop cap.
    "delta_to_events",
    // Private to the `reify-gui` BINARY, while this file is a `pub mod` of
    // the LIBRARY — so NOT reachable today. Kept: it fires the day that fn is
    // hoisted.
    "emit_delta",
];

/// True when the top-level fn `name` emits state itself rather than leaving
/// emission to the shared seam — it names one of [`PRIVATE_EMIT_IDENTIFIERS`]
/// or makes any `.emit(` call — in its own body or in one it delegates to,
/// following the SAME one hop [`reaches_a_seam`] does.
///
/// The `.emit(` arm is FORWARD-LOOKING: it needs an `Emitter` value in scope,
/// and `DebugServerState` (fields `engine`, `selection`, `debug_bridge`,
/// `last_state`) cannot supply one until an `AppHandle` is added to it.
fn emits_privately(code: &str, name: &str) -> bool {
    within_one_hop(code, name, |body, _| {
        identifiers(body).any(|id| PRIVATE_EMIT_IDENTIFIERS.contains(&id))
            || body.contains(".emit(")
    })
}

/// Every maximal `[A-Za-z0-9_]+` run in `text`.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
}

/// Appends `span` to `out` as an equal-length run of ASCII spaces, keeping
/// newlines, so byte offsets, line count and line lengths all survive
/// unchanged — `fn_body`'s column-0 `}` sentinel therefore still means the
/// same thing in a blanked view as in the source.
fn push_blanked(out: &mut String, span: &str) {
    for c in span.chars() {
        out.push(if c == '\n' { '\n' } else { ' ' });
        for _ in 1..c.len_utf8() {
            out.push(' ');
        }
    }
}

/// The byte just past the char literal starting at `at`, or `None` when that
/// `'` opens a LIFETIME (`&'static str`) instead.
///
/// Ported from `crates/reify-builtins/tests/common/seed_name_scan.rs`, whose
/// reason applies verbatim: a quote inside a char literal is not a quote to
/// any walk over the result. `'"'` would otherwise open a string here and
/// blank every byte to the next quote.
fn char_literal_end(raw: &[u8], at: usize) -> Option<usize> {
    let first = at + 1;
    if first >= raw.len() {
        return None;
    }
    if raw[first] == b'\\' {
        // An escape's payload can itself be a quote (`'\''`) or run several
        // bytes (`'\u{7b}'`), so step past it and find the real closing quote.
        let mut j = first + 2;
        while j < raw.len() && raw[j] != b'\'' {
            j += 1;
        }
        return (j < raw.len()).then_some(j + 1);
    }
    let width = match raw[first] {
        b if b < 0x80 => 1,
        b if b >> 5 == 0b110 => 2,
        b if b >> 4 == 0b1110 => 3,
        _ => 4,
    };
    let close = first + width;
    (close < raw.len() && raw[close] == b'\'').then_some(close + 1)
}

/// Blanks every `//`/`///`/`//!` line comment and `/* … */` block comment —
/// and, when `blank_literals`, the CONTENTS of every `"…"` string literal.
///
/// Blanking must be genuinely correct rather than merely conservative, because
/// the checks it feeds fail in OPPOSITE directions: leaving prose in place
/// false-GREENS the seam check (a seam NAMED in a comment or a log message
/// reads as routing), while blanking too much false-GREENS the private-emit
/// check.
///
/// It is a scanner, not a Rust lexer: it tracks `"` string literals — the only
/// literal in this corpus that can contain a `//` (`"http://…"`) — with `\`
/// escapes, across line boundaries, and steps over char literals so a `'"'`
/// cannot open one. Raw strings (`r"…"`, `r#"…"#`), which `debug_server.rs`
/// does not use, are still not special-cased.
///
/// KNOWN SPOT COST: this is the third hand-rolled Rust source scanner in the
/// repo, beside `crates/reify-eval/tests/version_id_discipline_gate.rs` and
/// `crates/reify-builtins/tests/common/seed_name_scan.rs`, each with its own
/// separately-documented blind spots — which is how the literal hole this fn
/// now closes survived here while `seed_name_scan.rs` already handled it.
/// Extracting the shared primitives spans three crates and so is not this
/// task's to make; filed as a follow-up.
fn blank_noncode(source: &str, blank_literals: bool) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut in_string = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let width = source[i..].chars().next().map_or(1, char::len_utf8);
        if in_string {
            if rest[0] == b'"' {
                // The delimiters are code; only what sits between them is prose.
                in_string = false;
                out.push('"');
                i += 1;
                continue;
            }
            // A backslash and the char it escapes move together, so an escaped
            // quote (`\"`) can neither close the string nor be split in half.
            let span = if rest[0] == b'\\' {
                1 + source[i + 1..].chars().next().map_or(0, char::len_utf8)
            } else {
                width
            };
            if blank_literals {
                push_blanked(&mut out, &source[i..i + span]);
            } else {
                out.push_str(&source[i..i + span]);
            }
            i += span;
            continue;
        }
        if rest.starts_with(b"//") {
            let end = source[i..].find('\n').map_or(source.len(), |n| i + n);
            push_blanked(&mut out, &source[i..end]);
            i = end;
            continue;
        }
        if rest.starts_with(b"/*") {
            let end = source[i..]
                .find("*/")
                .map_or(source.len(), |n| i + n + "*/".len());
            push_blanked(&mut out, &source[i..end]);
            i = end;
            continue;
        }
        if rest[0] == b'\''
            && let Some(end) = char_literal_end(bytes, i)
        {
            // Copied whole, never interpreted: one char can hold no
            // identifier and no `.emit(`, but it CAN hold a `"`.
            out.push_str(&source[i..end]);
            i = end;
            continue;
        }
        if rest[0] == b'"' {
            in_string = true;
        }
        out.push_str(&source[i..i + width]);
        i += width;
    }
    out
}

/// Comments blanked, string literals INTACT — the view the two tool-set
/// enumerations need, since both read tool names OUT of literals.
fn strip_comments(source: &str) -> String {
    blank_noncode(source, false)
}

/// Comments and string-literal contents both blanked — the view every
/// BEHAVIOURAL check reads, so a seam or an emit named only in prose (a doc
/// comment, a tracing message, a `json!` field) can never read as a call.
///
/// Both views are equal-length blankings of the same bytes, so an offset means
/// the same thing in either and [`fn_body`] slices line up across them.
fn strip_prose(source: &str) -> String {
    blank_noncode(source, true)
}

/// Names every top-level fn in `code` whose name ends `_and_refresh_baseline`,
/// in source order. `code` must already be comment-stripped.
fn seam_fns(code: &str) -> Vec<&str> {
    code.lines()
        .filter_map(|line| parse_fn_name(line.trim_end()))
        .filter(|name| name.ends_with("_and_refresh_baseline"))
        .collect()
}

/// Names the `*_and_refresh_baseline` fns that never actually refresh the
/// baseline, sorted.
///
/// Without this, [`write_tool_bypasses`] would rest on a NAME: any fn called
/// `…_and_refresh_baseline` would satisfy it, so a seam whose name lies would
/// green the gate falsely. A seam is accepted if its body reaches
/// `compute_delta` directly, or — one hop, same cap and same fail-closed
/// direction as [`reaches_a_seam`] — if it delegates to another
/// `*_and_refresh_baseline` fn in the same source that does.
fn unrefreshing_seams(source: &str) -> Vec<String> {
    let code = strip_prose(source);
    let refreshes = |name: &str| {
        fn_body(&code, name).is_some_and(|body| identifiers(body).any(|id| id == "compute_delta"))
    };
    let mut liars: Vec<String> = seam_fns(&code)
        .into_iter()
        .filter(|name| {
            !refreshes(name)
                && !fn_body(&code, name).is_some_and(|body| {
                    identifiers(body).any(|id| {
                        id != *name && id.ends_with("_and_refresh_baseline") && refreshes(id)
                    })
                })
        })
        .map(str::to_string)
        .collect();
    liars.sort();
    liars
}

/// Every INV-GUI-2 violation in `source`, one per (write tool, defect).
fn write_tool_bypasses(source: &str) -> Vec<Bypass> {
    // Both views are derived ONCE here, at the single entry point, so no
    // helper below can independently forget to blank — or read the wrong view.
    // The enumerations read tool names out of literals; the behavioural checks
    // must not see them at all.
    let code = strip_comments(source);
    let prose_free = strip_prose(source);
    let mut bypasses: Vec<Bypass> = dispatch_arms(&code)
        .into_iter()
        .flat_map(|(tool, handler)| {
            // RESOLUTION IS NOT NAME-MATCHING: a handler that is not a
            // top-level fn of this file is reported as unresolved, not
            // mislabelled as one that skips the seam.
            let kinds = match fn_body(&prose_free, &handler) {
                None => vec![BypassKind::UnresolvedHandler],
                // The two defects are INDEPENDENT: a handler can route
                // correctly and still emit privately, and collapsing them
                // would hide one.
                Some(_) => [
                    (!reaches_a_seam(&prose_free, &handler))
                        .then_some(BypassKind::NoBaselineRefresh),
                    emits_privately(&prose_free, &handler).then_some(BypassKind::PrivateEmit),
                ]
                .into_iter()
                .flatten()
                .collect(),
            };
            kinds.into_iter().map(move |kind| Bypass {
                tool: tool.clone(),
                handler: handler.clone(),
                kind,
            })
        })
        .collect();
    // Sorted so failure output is stable across runs; `Bypass`'s derived Ord
    // is (tool, handler, kind).
    bypasses.sort();
    bypasses
}

/// Reads the real `debug_server.rs` this gate is asserted against.
///
/// Located via [`super::gui_crate_manifest_dir`], which prefers the RUNTIME
/// `CARGO_MANIFEST_DIR` over the compile-time `env!()` bake — the bake goes
/// stale when a seeded warm-lane `target/` is reused from a since-deleted
/// worktree (esc-4906-57).
fn debug_server_source() -> String {
    let path = std::path::Path::new(&super::gui_crate_manifest_dir()).join("src/debug_server.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} for the INV-GUI-2 gate: {e}",
            path.display()
        )
    })
}

/// The break-glass env knob, named ONCE so no branch and no message can drift
/// onto a different spelling of it.
const BYPASS_ENV: &str = "REIFY_INV_GUI_2_BYPASS";

/// What the corpus sweep decided, as a VALUE rather than as control flow.
///
/// As branches the downgrade path would ship unexercised — it runs only when
/// the real file is dirty, never on a green tree. As a value,
/// [`the_break_glass_knob_downgrades_a_real_bypass_to_a_warn`] drives every
/// arm against a fixture on every run.
#[derive(Debug, PartialEq, Eq)]
enum SweepOutcome {
    Clean,
    Warn(String),
    Fail(String),
}

/// The sweep's verdict on `source`, pure over the [`BYPASS_ENV`] VALUE so the
/// decision is testable without mutating process env — which would race every
/// other test sharing this binary.
///
/// Only the exact value `1` arms the downgrade, mirroring
/// `REIFY_MAIN_GATE_BYPASS`; a knob set to anything else still fails.
fn sweep_outcome(source: &str, bypass_env: Option<&str>) -> SweepOutcome {
    let bypasses = write_tool_bypasses(source);
    if bypasses.is_empty() {
        return SweepOutcome::Clean;
    }
    let report = std::iter::once(format!(
        "{} `reify_*` write tool(s) do not reach the delta baseline through one of the two \
         shared `*_and_refresh_baseline` seams:",
        bypasses.len()
    ))
    .chain(bypasses.iter().map(|b| {
        format!(
            "  {} -> {} ({:?}) in gui/src-tauri/src/debug_server.rs",
            b.tool, b.handler, b.kind
        )
    }))
    .collect::<Vec<_>>()
    .join("\n");
    if bypass_env == Some("1") {
        SweepOutcome::Warn(report)
    } else {
        SweepOutcome::Fail(report)
    }
}

#[test]
fn bypassing_fixture_is_flagged() {
    assert_eq!(
        write_tool_bypasses(BYPASSING_SOURCE),
        vec![Bypass {
            tool: "reify_set_parameter".to_string(),
            handler: "handle_reify_set_parameter".to_string(),
            kind: BypassKind::NoBaselineRefresh,
        }],
    );
}

#[test]
fn seam_named_only_in_a_comment_does_not_count() {
    assert_eq!(
        write_tool_bypasses(COMMENT_ONLY_MENTION_SOURCE),
        vec![Bypass {
            tool: "reify_export".to_string(),
            handler: "handle_reify_export".to_string(),
            kind: BypassKind::NoBaselineRefresh,
        }],
    );
}

#[test]
fn seam_named_only_in_a_string_does_not_count() {
    assert_eq!(
        write_tool_bypasses(STRING_ONLY_MENTION_SOURCE),
        vec![Bypass {
            tool: "reify_save_file".to_string(),
            handler: "handle_reify_save_file".to_string(),
            kind: BypassKind::NoBaselineRefresh,
        }],
    );
}

/// [`blank_noncode`]'s two non-obvious claims, pinned on the primitive itself
/// rather than only through the fixtures that ride on it: a `//` INSIDE a
/// string literal does not start a comment, and a `\"` escape does not close
/// the string. A desync in either scan blanks arbitrarily much following CODE,
/// which false-GREENS every downstream check in silence — the direction no
/// fixture over a well-formed handler can reach.
#[test]
fn the_string_scan_survives_slashes_and_escapes() {
    // The trailing call sits on the SAME line as the literal deliberately: a
    // scan that mistook the `//` for a comment would blank to end of LINE, so
    // putting the call on the next line would let both assertions pass even
    // with the string tracking removed entirely.
    let url = r#"    let u = "http://x//y"; state.app.emit("d", &d);
"#;
    assert!(strip_comments(url).contains(".emit("));
    assert!(strip_comments(url).contains("http://x//y"));

    let escaped = r#"    let s = "a \" // b"; state.app.emit("d", &d);
"#;
    assert!(strip_comments(escaped).contains(".emit("));

    // A `"` inside a CHAR literal must not open a string. Blanking from there
    // to the next quote swallows real code, and swallowed code false-GREENS
    // the private-emit check — not merely noise, the forbidden direction.
    let quote_char = r#"    let q = '"'; state.app.emit("d", &d);
"#;
    assert!(strip_comments(quote_char).contains(".emit("));
    assert!(strip_prose(quote_char).contains(".emit("));

    // A lifetime is NOT a char literal, so recognising `'` must not swallow
    // the rest of `&'static str` either.
    let lifetime = r#"    let n: &'static str = "x"; state.app.emit("d", &d);
"#;
    assert!(strip_comments(lifetime).contains(".emit("));

    // Blanking is equal-LENGTH, which is what lets `fn_body` slice one view
    // with offsets found in another: the comment's text is gone, the code
    // around it is untouched, and every byte offset still means what it did.
    let commented = r#"fn a() {
    // emit_delta(&state.app, &delta);
    let x = 1;
}
"#;
    let code = strip_comments(commented);
    assert_eq!(code.len(), commented.len());
    assert!(!code.contains("emit_delta"));
    assert!(code.contains("let x = 1;"));

    // The prose-free view additionally empties the literals it keeps in place.
    let prose_free = strip_prose(url);
    assert_eq!(prose_free.len(), url.len());
    assert!(prose_free.contains(".emit("));
    assert!(!prose_free.contains("http"));
}

#[test]
fn compliant_fixtures_in_both_seam_shapes_are_clean() {
    assert_eq!(write_tool_bypasses(COMPLIANT_SOURCE), vec![]);
}

/// THE gate: the mechanized corpus sweep against the real `debug_server.rs`.
#[test]
fn every_debug_write_tool_routes_through_the_delta_choke_point() {
    let source = debug_server_source();

    // NON-VACUITY FLOOR — checked first so a moved file, a stopped parser, or
    // the write-tool cluster moving to its own module (weighed under
    // `debug_server.rs`'s "WHY THE CLUSTER LIVES IN THIS FILE" note) reds
    // instead of passing vacuously.
    //
    // Anchored on the REGISTRY, not the arms, which is what makes the headroom
    // sound: a broken arm parser empties the arms while the registry still
    // reads five, so the set-difference assertion below fires first and only a
    // gross REGISTRY failure reaches this floor. Pinned instead to today's
    // exact count, it would false-red a legitimate tool REMOVAL while still
    // doing nothing about a dropped sixth tool.
    let advertised = registry_tool_names(&strip_comments(&source));
    assert!(
        advertised.len() >= 3,
        "ToolDef-registry scan of debug_server.rs found only {} `reify_*` tool(s) (expected \
         >= 3) — the checker may be reading the wrong file, the registry parser may have \
         stopped matching, or the write tools may have moved to another module",
        advertised.len()
    );

    // COMPLETENESS — strictly orthogonal to the floor above (one assertion,
    // one job): the floor catches gross breakage, this catches ONE tool going
    // missing regardless of count, and it is what makes "a new write tool
    // cannot skip this gate silently" actually true.
    assert_eq!(
        unenumerated_tools(&source),
        Vec::<String>::new(),
        "the ToolDef registry advertises `reify_*` write tool(s) the dispatch-arm scan never \
         saw, so NEITHER half of INV-GUI-2 would sweep them: either the arm is in a shape the \
         scanner cannot read, or the tool is dispatched by a non-literal path and falls into \
         the `_ =>` frontend-delegation catch-all"
    );

    match sweep_outcome(&source, std::env::var(BYPASS_ENV).ok().as_deref()) {
        SweepOutcome::Clean => {}
        SweepOutcome::Warn(report) => eprintln!("[{BYPASS_ENV}] DOWNGRADED to warn:\n{report}"),
        SweepOutcome::Fail(report) => panic!(
            "INV-GUI-2: {report}\n\n(set {BYPASS_ENV}=1 to downgrade this to a warning as a \
             break-glass escape hatch)"
        ),
    }
}

#[test]
fn no_write_tool_handler_emits_privately() {
    assert_eq!(
        write_tool_bypasses(PRIVATE_EMIT_SOURCE),
        vec![Bypass {
            tool: "reify_update_source".to_string(),
            handler: "handle_reify_update_source".to_string(),
            kind: BypassKind::PrivateEmit,
        }],
    );

    // The real file is clean, and is only clean because prose is blanked
    // first: its ONLY textual `emit_delta` occurrences are the shared
    // INV-GUI-2 rationale banner above the two seams and point (b) on
    // `write_on_engine_and_refresh_baseline`, both comments, so a checker
    // reading raw text would false-positive right here.
    let private_emits: Vec<Bypass> = write_tool_bypasses(&debug_server_source())
        .into_iter()
        .filter(|b| b.kind == BypassKind::PrivateEmit)
        .collect();
    assert_eq!(private_emits, vec![]);
}

/// The private-emit sweep follows the SAME one delegation hop the seam sweep
/// does. Without that, the asymmetry silently exempted `reify_open_file`,
/// whose handler delegates its whole body away.
#[test]
fn a_private_emit_in_the_delegated_helper_is_flagged() {
    assert_eq!(
        write_tool_bypasses(DELEGATED_PRIVATE_EMIT_SOURCE),
        vec![Bypass {
            tool: "reify_open_file".to_string(),
            handler: "handle_reify_open_file".to_string(),
            kind: BypassKind::PrivateEmit,
        }],
    );
}

/// The grammar must span the library's REAL emission surface, not just the
/// binary-private `emit_delta` and a bare `.emit(`. A handler that routes
/// correctly and then emits again through `crate::event_bus::emit_typed` is a
/// second emission path exactly as much as a direct `app.emit` is — and it is
/// the only one of the three a `debug_server.rs` author can actually reach
/// from the library today.
#[test]
fn a_private_emit_through_the_event_bus_wrapper_is_flagged() {
    assert_eq!(
        write_tool_bypasses(EVENT_BUS_PRIVATE_EMIT_SOURCE),
        vec![Bypass {
            tool: "reify_set_parameter".to_string(),
            handler: "handle_reify_set_parameter".to_string(),
            kind: BypassKind::PrivateEmit,
        }],
    );
}

/// The NON-VACUITY FLOOR for the private-emit half, as a POSITIVE CONTROL.
///
/// Every other real-file assertion here is preceded by a floor — a registry
/// count, a seam count, a registry-vs-arm set difference. This one was not,
/// and it is precisely the one that went vacuous: its grammar named only
/// symbols no library module could reach, so "the real file has no private
/// emit" was true of every possible file. A count floor cannot help where the
/// correct answer is zero, so the file is MUTATED instead and the sweep must
/// notice — the in-suite form of the mutation a reviewer otherwise has to run
/// by hand. This closes the class, not the one instance.
#[test]
fn the_private_emit_sweep_fires_on_the_real_file_when_mutated() {
    let private_emits = |source: &str| -> Vec<Bypass> {
        write_tool_bypasses(source)
            .into_iter()
            .filter(|b| b.kind == BypassKind::PrivateEmit)
            .collect()
    };
    let source = debug_server_source();

    // The control and its negative are read together: a sweep that fires on
    // the mutant says nothing unless it stays silent on the original.
    assert_eq!(private_emits(&source), vec![]);

    // The target is READ from the dispatch scan rather than hardcoded, so the
    // control follows a rename instead of silently ceasing to mutate anything.
    let (tool, handler) = dispatch_arms(&strip_comments(&source))
        .into_iter()
        .next()
        .expect("debug_server.rs advertises no `reify_*` dispatch arm to mutate");
    let mutated = splice_into_fn_body(
        &source,
        &handler,
        "    crate::event_bus::emit_typed(&state.app, \"state-delta\", &delta).ok();",
    )
    .expect("the positive control failed to splice — an unmutated copy would pass vacuously");

    assert_eq!(
        private_emits(&mutated),
        vec![Bypass {
            tool,
            handler,
            kind: BypassKind::PrivateEmit,
        }],
    );
}

#[test]
fn every_refresh_baseline_seam_actually_refreshes() {
    assert_eq!(
        unrefreshing_seams(LYING_SEAM_SOURCE),
        vec!["reify_export_on_engine_and_refresh_baseline".to_string()],
    );

    let source = debug_server_source();

    // NON-VACUITY FLOOR — a renamed seam family or a broken fn-signature
    // parser must red here, not pass by finding nothing to check. Seven seams
    // exist today: write_on_engine_*, open_source_into_engine_* and
    // set_fea_case_on_engine_* reach compute_delta directly, and the four
    // reify_*_on_engine_* reach it via write_on_engine_and_refresh_baseline.
    let code = strip_comments(&source);
    let seams = seam_fns(&code);
    assert!(
        seams.len() >= 3,
        "seam scan of debug_server.rs found only {} `*_and_refresh_baseline` fn(s) \
         (expected >= 3) — the seam family may have been renamed, or the fn-signature \
         parser may have stopped matching: {seams:?}",
        seams.len()
    );

    assert_eq!(
        unrefreshing_seams(&source),
        Vec::<String>::new(),
        "a fn named `*_and_refresh_baseline` never reaches `crate::diff::compute_delta`, \
         so INV-GUI-2's routing check would be resting on that name rather than on behaviour"
    );
}

/// The break-glass hatch, actually exercised. Its branch only ever runs on a
/// RED tree, so without a fixture driving it the knob ships untested.
#[test]
fn the_break_glass_knob_downgrades_a_real_bypass_to_a_warn() {
    let SweepOutcome::Fail(report) = sweep_outcome(BYPASSING_SOURCE, None) else {
        panic!("an unset {BYPASS_ENV} must leave the sweep asserting");
    };
    assert!(report.contains("reify_set_parameter"), "{report}");

    // Armed: the same report, downgraded verdict.
    assert_eq!(
        sweep_outcome(BYPASSING_SOURCE, Some("1")),
        SweepOutcome::Warn(report),
    );

    // Anything but the exact value leaves the gate asserting, so a knob
    // someone half-set cannot silently disarm it.
    for set_to in ["0", "true", ""] {
        assert!(
            matches!(
                sweep_outcome(BYPASSING_SOURCE, Some(set_to)),
                SweepOutcome::Fail(_)
            ),
            "{BYPASS_ENV}={set_to:?} must not disarm the gate"
        );
    }

    // A clean corpus is Clean either way — the knob downgrades a failure, it
    // never suppresses the sweep itself.
    assert_eq!(
        sweep_outcome(COMPLIANT_SOURCE, Some("1")),
        SweepOutcome::Clean
    );
}

/// The symbols this module's doc comments cite must still exist in the file
/// they cite them from.
///
/// Line numbers were replaced by symbol anchors because names survive edits —
/// but only while they are still names, so this pins them rather than leaving
/// a drift-resistance gate resting on unchecked pointers. It doubles as a
/// non-vacuity check on [`fn_body`]: every anchor below is resolved with the
/// same primitive the gate itself runs on.
#[test]
fn every_anchor_this_module_cites_still_exists() {
    let source = debug_server_source();
    let code = strip_comments(&source);

    let missing: Vec<&str> = [
        "tool_defs",
        "dispatch_tool",
        "handle_reify_open_file",
        "open_path_into_engine",
        "open_source_into_engine_and_refresh_baseline",
        "write_on_engine_and_refresh_baseline",
    ]
    .into_iter()
    .filter(|name| fn_body(&code, name).is_none())
    .collect();
    assert_eq!(
        missing,
        Vec::<&str>::new(),
        "this module's prose cites top-level fn(s) `debug_server.rs` no longer defines — \
         re-point the prose at whatever replaced them (or `fn_body` has stopped resolving)"
    );
}

/// The soundness net for the arm scan: a tool the scan CANNOT SEE must red as
/// a set difference, never vanish. The registry is an independent textual
/// enumeration of the same set, so whatever the reason an arm is unreadable —
/// including reasons nobody has thought of — the tool still shows up here.
#[test]
fn advertised_tools_must_all_appear_in_the_dispatch_scan() {
    assert_eq!(
        unenumerated_tools(ADVERTISED_BUT_UNDISPATCHED_SOURCE),
        vec!["reify_beta".to_string()],
    );
}

/// Every ordinary arm shape must ENUMERATE. A shape the scan cannot read is
/// caught by the registry cross-check either way, but a gate that reds on
/// ordinary Rust invites the next author to weaken it.
#[test]
fn ordinary_arm_shapes_are_all_enumerated() {
    assert_eq!(
        dispatch_arms(&strip_comments(ARM_SHAPES_SOURCE)),
        vec![
            (
                "reify_wrapped".to_string(),
                "handle_reify_wrapped".to_string()
            ),
            (
                "reify_blocked".to_string(),
                "handle_reify_blocked".to_string()
            ),
            ("reify_first".to_string(), "handle_shared".to_string()),
            ("reify_second".to_string(), "handle_shared".to_string()),
        ],
    );

    // The anti-regression half: the two enumerations now AGREE on a fixture
    // where before they did not, so the widening removed the red rather than
    // moving it somewhere else.
    assert_eq!(unenumerated_tools(ARM_SHAPES_SOURCE), Vec::<String>::new());
}

/// A tool whose `"reify_*"` pattern the scan HAS seen is never dropped: if its
/// handler resolves to nothing the checker can find, that is what the report
/// says, rather than a confident claim about a seam the handler never had.
#[test]
fn an_arm_with_no_resolvable_handler_is_reported() {
    assert_eq!(
        write_tool_bypasses(INLINE_ARM_SOURCE),
        vec![Bypass {
            tool: "reify_inline".to_string(),
            handler: "Ok".to_string(),
            kind: BypassKind::UnresolvedHandler,
        }],
    );
}
