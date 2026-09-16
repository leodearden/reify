//! INV-GUI-2 (`docs/invariants.md`) structural gate for the AI/MCP entry
//! point: every `reify_*` write tool reaches the delta baseline through one
//! of the two shared `*_and_refresh_baseline` seams, never through a private
//! emit of its own.
//!
//! The claim this mechanizes is stated in prose in two places and is NOT
//! restated here — see `gui/src-tauri/src/debug_server.rs:1867-1908`, point
//! (a) on `write_on_engine_and_refresh_baseline`, and the "Two seams, ONE
//! stated exception" section of `docs/debug-mcp-contract.md`. Note the shape
//! of the claim: "one of the two shared seams", NOT "all five route through
//! `write_on_engine_and_refresh_baseline`" — `reify_open_file` reaches the
//! same refresh through `open_source_into_engine_and_refresh_baseline`, for
//! the #5193 lock-ordering reason point (d) gives.
//!
//! This reads `debug_server.rs` as TEXT and links nothing, so unlike
//! `debug_boundary_tests` — which builds a real `EngineSession` — it carries
//! NO `#[cfg(feature = "gui")]` gate. That is deliberate: it runs in the
//! DEFAULT test pass rather than only in `verify.sh`'s conditionally-emitted
//! `--features gui` arm, so the architecture gate cannot be silently skipped
//! on a run that does not touch the GUI crate — exactly the run during which
//! someone might add a bypassing tool elsewhere.
//!
//! REDUNDANT ENUMERATION: the write-tool set is read from TWO independent
//! textual shapes — the `"reify_*" =>` dispatch arms and the `ToolDef`
//! registry advertising the same names — which must agree. That is what makes
//! an arm the scanner cannot read fail CLOSED: the tool reds as a set
//! difference rather than vanishing from the sweep, whatever the reason the
//! arm was unreadable. The residual is stated rather than papered over — a
//! drop stays silent if BOTH enumerations miss the SAME tool in a correlated
//! way, and the registry-side non-vacuity floor is what catches the case where
//! both go to zero at once.
//!
//! POSTURE: ships default-ASSERT. The task called for contract → warn-mode
//! corpus sweep → enforce; the sweep was performed at plan time (5/5 routed,
//! clean) and is re-performed mechanically by
//! `every_debug_write_tool_routes_through_the_delta_choke_point` on every
//! run, so a warn-only default would emit no signal on a green tree and defer
//! the leaf indefinitely. `REIFY_INV_GUI_2_BYPASS=1` is the break-glass
//! escape hatch, mirroring `REIFY_MAIN_GATE_BYPASS` (CLAUDE.md), and it is
//! genuinely live: it lets someone mid-refactor land a legitimate third seam
//! by setting a knob instead of deleting the guard. A
//! `REIFY_INV_GUI_2_ENFORCE` alias is deliberately NOT provided — against a
//! default-assert shipped state it would be a no-op, and an unreachable knob
//! is worse than an absent one. Same posture convention as
//! `crates/reify-eval/tests/harness_cache/snapshot_cache_divergence_gate.rs`.

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
/// The write-tool set is ENUMERATED here rather than hardcoded, so a sixth
/// `reify_*` tool is picked up automatically and must route or go red — which
/// is the gap INV-GUI-2 exists to close. There is deliberately no read-only
/// exemption set: a future read-only `reify_*` tool reds until someone
/// classifies it consciously.
///
/// Once a tool's `"reify_*"` pattern literal has been SEEN it is never
/// dropped. An arm whose handler cannot be resolved still yields the tool,
/// paired with whatever the scan did resolve (possibly nothing), which
/// [`write_tool_bypasses`] reports as [`BypassKind::UnresolvedHandler`].
/// Dropping was this checker's one false-GREEN direction: a tool missing from
/// this list is swept by NEITHER half of the gate.
///
/// Retained approximation, in the module's fail-closed direction: the handler
/// is the FIRST `identifier(` call in the arm, so a block arm that calls
/// something else first — a guard, a `Box::pin` — resolves to the wrong fn and
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
/// order (`debug_server.rs:1026/1056/1084/1101/1115`). `code` must already be
/// comment-stripped.
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

/// Names the advertised `reify_*` tools the dispatch scan never saw, sorted.
///
/// REDUNDANT ENUMERATION: reading the tool set from two independent textual
/// shapes is what makes an arm the scanner cannot read fail CLOSED. Without
/// it, [`dispatch_arms`] silently DROPS such an arm and the tool vanishes from
/// the sweep entirely — a false GREEN, the one direction this module must
/// never have. Here it reds as a set difference instead, whatever the reason
/// the arm was unreadable, including reasons nobody has anticipated.
///
/// It also catches a shape no arm parser could reach at all: a
/// registry-advertised tool dispatched by a non-literal path, falling into the
/// `_ =>` frontend-delegation catch-all at `debug_server.rs:1329`.
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
///
/// The terminator is the next line that is exactly `}` in column 0. That is
/// sound for `debug_server.rs`, which indents every nested block, so no inner
/// brace can reach column 0; a top-level fn's own closing brace is the first
/// that does. A file violating that convention truncates the body early,
/// which loses seam matches and therefore false-POSITIVES — red, never
/// silently green.
fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
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
                    return Some(&source[begin..offset + line.len()]);
                }
            }
        }
        offset += line.len();
    }
    None
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

/// True when the top-level fn `name` emits state itself rather than leaving
/// emission to the shared seam — an `emit_delta` identifier, or any `.emit(`
/// call — in its own body or in one it delegates to.
fn emits_privately(code: &str, name: &str) -> bool {
    within_one_hop(code, name, |body, _| {
        identifiers(body).any(|id| id == "emit_delta") || body.contains(".emit(")
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
/// escapes, across line boundaries. Raw strings (`r"…"`, `r#"…"#`) and the
/// pathological `'"'` char literal, neither of which occurs in
/// `debug_server.rs`, are not special-cased.
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
/// Expressed as branches, the downgrade path would ship entirely unexercised:
/// it only runs when the real file is dirty, which is never on a green tree,
/// so an inverted condition or a typo'd env name would first be discovered by
/// whoever needs the hatch mid-incident. As a value,
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

/// A synthetic `debug_server.rs` excerpt whose `reify_set_parameter` handler
/// mutates the engine directly and pushes to the frontend without ever
/// reaching a `*_and_refresh_baseline` seam. This is the defect INV-GUI-2
/// exists to catch; the checker must flag it.
const BYPASSING_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "engine_state" => handle_engine_state(state).await,
        "reify_set_parameter" => handle_reify_set_parameter(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_set_parameter(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let (cell_id, value) = reify_set_parameter_params(&params)?;
    let gs = run_on_engine(&state.engine, move |s| {
        s.apply_param_to_source_str(&cell_id, &value)
    })
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_set_parameter_envelope(None, None, vec![]))
}
"#;

/// A fixture whose `handle_reify_export` NAMES both seams in prose — a doc
/// comment and a line comment — while its body bypasses them entirely.
///
/// Not hypothetical: the real `handle_reify_set_parameter`'s doc comment
/// literally contains "1. `reify_set_parameter_on_engine_and_refresh_baseline`
/// — splices the …". A checker that greps raw text greens any handler that
/// merely mentions a seam while bypassing it in code.
const COMMENT_ONLY_MENTION_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_export" => handle_reify_export(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

/// Export the current model.
///
/// Flow:
///  1. `reify_export_on_engine_and_refresh_baseline` — writes the file and
///     refreshes the delta baseline.
///  2. push the rebuilt `GuiState` to the frontend.
async fn handle_reify_export(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let (format, output_path) = reify_export_params(&params)?;
    // routes through write_on_engine_and_refresh_baseline
    let gs = run_on_engine(&state.engine, move |s| s.export(&format, &output_path)).await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_export_envelope(&output_path))
}
"#;

/// A fixture reaching the same hole as [`COMMENT_ONLY_MENTION_SOURCE`]
/// through STRING LITERALS instead of prose, in both directions at once:
///
/// - `handle_reify_save_file` names a seam only inside a `tracing` message
///   while bypassing it in code — a false GREEN, the direction this module
///   must never have. A span name, an error string or a `json!` field naming
///   the seam being removed is an ordinary edit.
/// - `handle_reify_export` routes correctly and merely mentions `.emit(` in a
///   log string — a false RED, merely noisy, but fixed by the same blanking.
const STRING_ONLY_MENTION_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_save_file" => handle_reify_save_file(state, params).await,
        "reify_export" => handle_reify_export(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_save_file(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let path = reify_save_file_params(&params)?;
    tracing::debug!("bypassing write_on_engine_and_refresh_baseline for speed");
    let gs = run_on_engine(&state.engine, move |s| s.save_to(&path)).await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_save_file_envelope(&path))
}

async fn handle_reify_export(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let (format, output_path) = reify_export_params(&params)?;
    let gs = write_on_engine_and_refresh_baseline(&state.engine, &state.last_state, move |s| {
        s.export(&format, &output_path)
    })
    .await?;
    tracing::debug!("the frontend push replaced a state.app.emit( call here");
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_export_envelope(&output_path))
}
"#;

/// Both compliant shapes the real file uses, side by side:
///
/// - `reify_set_parameter` names its seam directly in the handler body — the
///   shape four of the five write tools take.
/// - `reify_open_file` reaches the SAME refresh ONE HOP away, through
///   `open_path_into_engine` (debug_server.rs:1701 → :1525 → :1640). This is
///   the one stated exception δ documented for θ, and the only shape that
///   forces the checker to trace a delegation.
const COMPLIANT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_open_file" => handle_reify_open_file(state, params).await,
        "reify_set_parameter" => handle_reify_set_parameter(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_open_file(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let raw_path = open_file_path_param(&params)?;
    let (frontend, content) = open_path_into_engine(state, &raw_path).await?;
    frontend_ok(frontend, "open_file")?;
    Ok(reify_open_file_envelope(&content))
}

async fn open_path_into_engine(
    state: &DebugServerState,
    raw_path: &str,
) -> Result<(Value, String), String> {
    let path = canonicalize_open_path(raw_path)?;
    let gui_state =
        open_source_into_engine_and_refresh_baseline(&state.engine, &state.last_state, &path)
            .await?;
    let frontend = push_gui_state(&state.debug_bridge, &gui_state, None).await?;
    Ok((frontend, gui_state.source.clone()))
}

async fn handle_reify_set_parameter(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let (cell_id, value) = reify_set_parameter_params(&params)?;
    let gs = reify_set_parameter_on_engine_and_refresh_baseline(
        &state.engine,
        &state.last_state,
        &cell_id,
        &value,
    )
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_set_parameter_envelope(None, None, vec![]))
}
"#;

/// A handler that DOES route through a seam but then emits a second time on
/// its own — the private emission path `debug_server.rs:1888` forbids in as
/// many words: "Do NOT add a second emit path here or in any caller".
const PRIVATE_EMIT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_update_source" => handle_reify_update_source(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_update_source(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let source = reify_update_source_params(&params)?;
    let gs = reify_update_source_on_engine_and_refresh_baseline(
        &state.engine,
        &state.last_state,
        &source,
    )
    .await?;
    let delta = crate::diff::compute_delta(&state.last_state, &gs);
    emit_delta(&state.app, &delta);
    state.app.emit("state-delta", &delta).ok();
    Ok(reify_update_source_envelope(&gs))
}
"#;

/// A handler that is itself spotless and delegates everything to a helper
/// that emits privately — the REAL `handle_reify_open_file` shape, which does
/// nothing but call `open_path_into_engine`.
///
/// So while `reaches_a_seam` followed that hop and `emits_privately` did not,
/// the one tool the whole delegation machinery exists for had its entire
/// emission behaviour outside the sweep: an `app.emit(…)` added to
/// `open_path_into_engine` left the gate green.
const DELEGATED_PRIVATE_EMIT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_open_file" => handle_reify_open_file(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_open_file(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let raw_path = open_file_path_param(&params)?;
    let (frontend, content) = open_path_into_engine(state, &raw_path).await?;
    frontend_ok(frontend, "open_file")?;
    Ok(reify_open_file_envelope(&content))
}

async fn open_path_into_engine(
    state: &DebugServerState,
    raw_path: &str,
) -> Result<(Value, String), String> {
    let path = canonicalize_open_path(raw_path)?;
    let gui_state =
        open_source_into_engine_and_refresh_baseline(&state.engine, &state.last_state, &path)
            .await?;
    let delta = crate::diff::compute_delta(&state.last_state, &gui_state);
    state.app.emit("state-delta", &delta).ok();
    let frontend = push_gui_state(&state.debug_bridge, &gui_state, None).await?;
    Ok((frontend, gui_state.source.clone()))
}
"#;

/// A handler that looks perfectly compliant — it calls a
/// `*_and_refresh_baseline` fn — where that fn never reaches `compute_delta`.
/// Without this check the whole gate would rest on a NAME rather than on
/// behaviour, and a seam whose name lies would green it falsely.
const LYING_SEAM_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_export" => handle_reify_export(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_export(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let (format, output_path) = reify_export_params(&params)?;
    let gs = reify_export_on_engine_and_refresh_baseline(
        &state.engine,
        &state.last_state,
        &format,
        &output_path,
    )
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_export_envelope(&output_path))
}

pub async fn reify_export_on_engine_and_refresh_baseline(
    engine: &Arc<Mutex<EngineSession>>,
    last_state: &std::sync::Mutex<Option<crate::types::GuiState>>,
    format: &str,
    output_path: &str,
) -> Result<crate::types::GuiState, String> {
    let format = format.to_owned();
    let output_path = output_path.to_owned();
    let _ = last_state;
    run_on_engine(engine, move |s| {
        let fmt = crate::commands::parse_export_format(&format)?;
        s.export(fmt, std::path::Path::new(&output_path))?;
        s.build_gui_state()
    })
    .await
}
"#;

/// A fixture whose `ToolDef` registry advertises BOTH `reify_alpha` and
/// `reify_beta` while `dispatch_tool` carries a literal arm for `reify_alpha`
/// only — `reify_beta` falls through to the `_ =>` frontend-delegation
/// catch-all, the shape the live `dispatch_tool` has at
/// `debug_server.rs:1329`, so it is dispatched but never seen by the arm scan.
///
/// No widening of the arm parser can reach this shape: there is no arm to
/// read. Only a second, independent enumeration of the tool set catches it.
const ADVERTISED_BUT_UNDISPATCHED_SOURCE: &str = r#"
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "reify_alpha",
            description: "Set alpha.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_beta",
            description: "Set beta.",
            input_schema: json!({ "type": "object" }),
        },
    ]
}

async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_alpha" => handle_reify_alpha(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_alpha(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let value = reify_alpha_params(&params)?;
    let gs = write_on_engine_and_refresh_baseline(&state.engine, &state.last_state, move |s| {
        s.set_alpha(&value)
    })
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_alpha_envelope(&gs))
}
"#;

/// The three shapes of ordinary Rust the review's probe confirmed the original
/// per-line arm parser DROPPED — wrapped, block and or-pattern — each
/// advertised in a matching `ToolDef` entry so the fixture exercises the
/// registry cross-check alongside the scan.
///
/// With that cross-check in place these shapes already fail CLOSED, so
/// widening the parser is false-POSITIVE reduction rather than a soundness
/// fix. It is still worth doing: a gate that reds on ordinary Rust is a gate
/// the next author weakens to get past. The or-pattern matters twice over —
/// dropping one silently drops BOTH names.
const ARM_SHAPES_SOURCE: &str = r#"
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "reify_wrapped",
            description: "Handler on the following line.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_blocked",
            description: "Handler inside a block arm.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_first",
            description: "Shares one handler with reify_second.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_second",
            description: "Shares one handler with reify_first.",
            input_schema: json!({ "type": "object" }),
        },
    ]
}

async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_wrapped" =>
            handle_reify_wrapped(state, params).await,
        "reify_blocked" => { handle_reify_blocked(state, params).await }
        "reify_first" | "reify_second" => handle_shared(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}
"#;

/// A tool handled INLINE, with no handler fn anywhere in the file.
///
/// The trap: `Ok` IS an identifier followed by `(`, so a first-call-expression
/// rule resolves the handler to `Ok` and then reports `NoBaselineRefresh` —
/// telling the reader the handler skips the seam when the truth is that the
/// checker never found a handler at all. Those two warrant different fixes, so
/// they get different names.
const INLINE_ARM_SOURCE: &str = r#"
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "reify_inline",
            description: "Answered inline, with no handler fn.",
            input_schema: json!({ "type": "object" }),
        },
    ]
}

async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_inline" => Ok(Value::Null),
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}
"#;

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

    // NON-VACUITY FLOOR — checked before the real assertion so a moved file
    // or a parser that silently stopped matching reds instead of passing
    // vacuously (same shape as `every_test_module_file_is_declared`'s floor).
    // It also fail-closes the split debug_server.rs weighed under "WHY THE
    // CLUSTER LIVES IN THIS FILE" (:1987-2006): if
    // the write-tool cluster moves to its own module the registry vanishes
    // here, this fires, and someone must re-point the checker.
    //
    // Anchored on the REGISTRY rather than on the arms, which is what makes
    // the headroom sound: if the arm parser breaks the arms go empty while the
    // registry still reads five, so the set-difference assertion below fires
    // first and only a gross REGISTRY-parser failure ever reaches this floor.
    // The headroom is a fix in its own right — pinned to today's exact count,
    // this floor would false-red a legitimate tool REMOVAL while still doing
    // nothing about a dropped sixth tool, which is what it was doing before.
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

    // The real file is clean, and is only clean because step-4 strips
    // comments: its ONLY textual `emit_delta` occurrences are the doc
    // comments at :1612 and :1888, so a checker reading raw text would
    // false-positive right here.
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
