//! Repo-wide fence gate for the MCP language-reference chunks
//! (`crates/reify-mcp/src/tools/chunks/*.md`).
//!
//! Task #5479 (PRD `docs/prds/v0_6/doc-chunk-truth-enforcement.md`, leaf β).
//!
//! # What this gate enforces
//!
//! 1. Every fence tagged EXACTLY ```` ```reify ```` compiles as a complete
//!    module with zero `Severity::Error` diagnostics.
//! 2. No fence is untagged. A bare opening delimiter is a violation naming
//!    `file:line`. Backtick and tilde (`~~~`) fences both count — CommonMark
//!    allows either, so a backtick-only scan would leave a silent hole.
//! 3. Every fence tagged EXACTLY ```` ```reify-invalid ```` genuinely FAILS to
//!    compile. The inverse of 1, over the one exempt tag that makes a
//!    falsifiable claim.
//! 4. Every `chunks/*.md` file on disk is reachable through the
//!    `reify_language_reference` MCP tool — i.e. both `include_str!`-ed AND
//!    listed in `TOPICS`.
//!
//! # Tag vocabulary
//!
//! - ```` ```reify ```` — **compiles STANDALONE as a complete module**. This is
//!   the one tag the gate compiles, and the meaning is deliberately the strong
//!   one: what the reader sees in the doc is exactly what the compiler was
//!   handed. No harness-side wrapper is applied, because a wrapper is an
//!   invisible privilege a reader of the doc cannot see.
//! - ```` ```reify-fragment ```` — genuine reify syntax that is member-level or
//!   otherwise context-dependent, so it cannot stand alone (a bare `let`, a
//!   `constraint`, a trait-body `fn` signature). A future content task could
//!   supply the missing context and PROMOTE it to ```` ```reify ````.
//! - ```` ```reify-schematic ```` — not reify source at all: signature
//!   listings, metavariable notation, `{ ... }` elisions. Never promotable.
//! - ```` ```reify-invalid ```` — a DELIBERATE-error teaching sample. Distinct
//!   from `reify-fragment`, which would falsely assert the body is valid. This
//!   is the one exempt tag the gate still VERIFIES (check 3): the body is
//!   compiled and a CLEAN compile is the violation. Without that, the tag would
//!   be the vocabulary's free downgrade path — a `reify` fence that stopped
//!   compiling could be made green by a one-line retag with nothing noticing.
//!   The other two exempt tags make claims no gate can falsify: checking
//!   `reify-fragment` would need an invisible harness-side wrapper, which is
//!   exactly what the bare `reify` tag's meaning was written to reject, and
//!   `reify-schematic` is a claim about intent.
//! - anything else (`ebnf`, `text`, …) — a non-reify language, exempt like any
//!   other explicit tag.
//!
//! The exempt list is **OPEN by design**: the gate only ever asks "is the tag
//! exactly `reify`". A closed allow-list would force this task to predict every
//! notation a future chunk might need; instead the tag itself is the sanction,
//! because retagging a fence away from `reify` is a one-line diff a reviewer
//! sees and can challenge.
//!
//! # Cross-harness contract (read before retagging anything)
//!
//! Two sibling modules in this same compile unit already scrape these chunks,
//! and they disagreed about what ```` ```reify ```` means:
//!
//! - `geometry_chunk_smoke::reify_tagged_fences` matches
//!   ```` line.trim_end() == format!("```{tag}") ```` — BYTE-EXACT on
//!   the whole info string, so `reify-fragment`/`reify-schematic` can never
//!   false-match it — and `reify_tagged_fences_in_geometry_chunk_compile`
//!   compiles each hit VERBATIM behind its own anti-vacuity floor of
//!   `>= 4`, the EXACT live count of geometry.md's four bare
//!   ```` ```reify ```` fences. Retagging one therefore fails that suite
//!   LOUDLY, not silently.
//!   `geometry_chunk_retains_bare_reify_fences_for_the_sibling_smoke_suite`
//!   below pins the coupling anyway, so the retag is named as the cause in its
//!   own diff instead of being diagnosed from a count in another module.
//! - `enums_chunk_option_smoke.rs:106` selects fences TAG-AGNOSTICALLY via
//!   `strip_prefix("```")` and WRAPS each body in `structure def OptionDemo
//!   {{ … }}` (:132). Its comment at :96 explicitly defers tag discipline to
//!   this module by name.
//!
//! This gate settles the disagreement in favour of the standalone reading, so
//! `enums.md`'s `## Option Type` fence — which passes today only because of
//! that injected wrapper — is `reify-fragment`, not `reify`.
//!
//! # What this gate structurally CANNOT reach (do not read green as "verified")
//!
//! A fence gate sees fences. The two chunk claims most often cited as
//! overstating v1 are **unfenced prose**, so no tagging decision this task made
//! touches them and no cosmetic green arises from their still being wrong:
//!
//! - `collections.md:19` — the bullet advertising `fold`, `all`, `any`,
//!   `concat` (and `map`) as List operations.
//! - `functions.md:28` — the bullet "**Recursion permitted** (infinite
//!   recursion is a runtime error)".
//!
//! Both are markdown list items, not code blocks. They are #5393's to correct;
//! that task's own seam note (ii) expects to land after this gate. This module
//! deliberately does not touch them — a gate that quietly widened itself into
//! prose scanning would be asserting a coverage claim it cannot keep.
//!
//! `functions.md`'s `## Overloading` listing used to overstate v1 the same way,
//! with a 3-arg `rotate(geometry, axis, angle)` overload — the very phantom
//! `a_reify_fence_whose_body_calls_the_phantom_three_arg_rotate_is_reported`
//! plants below. It is `reify-schematic` (a signature listing, not compilable
//! source), so this gate exempts it by design and could not have caught it.
//! What closed it instead was renaming the example to `align`, a name no
//! builtin arity-gates, so copying the CALL form alone now fails as an
//! undefined function rather than as a misleading arity error on a real
//! builtin. The arity of every signature the chunk declares is now pinned
//! directly by `functions_chunk_overloading_smoke.rs` — a scan over chunk
//! text, not a fence gate, which is why that guard lives beside this one
//! rather than inside it.
//!
//! The third gap is `reify-fragment`, and it is WIDER than the tag's wording
//! admits. "Member-level or otherwise context-dependent" asserts that SOME
//! enclosing context would make the body parse — unverifiable here for the
//! reason given in the vocabulary above, so load-bearing on author judgement
//! alone. Two fences reached the corpus where NO context exists because the
//! FORM is not v1 syntax: `units.md`'s dimension aliases (no `^` operator in a
//! dimension expression) and `traits.md`'s composition line (a `trait`
//! declaration always carries a body). Both are now `reify-schematic` with the
//! real constraint spelled out beside them. That triage was fence-by-fence
//! against the parser and is NOT a property the gate maintains; a follow-up
//! filed from #5479 owns the rest. Until it lands, read `reify-fragment` as
//! "the author asserts this is real syntax", never as "the gate agrees".
//!
//! There is one more gap, and it runs the OTHER way — a fence can be compiled,
//! be green, and still name something that does not exist. An UNRESOLVED CALL
//! NAME is frequently not an error: a body types such a call from its first
//! argument's `result_type`, the same tolerance `geometry_chunk_smoke.rs`'s
//! scope statement warns about and the reason this module's phantom demo pins
//! an ARITY failure on the KNOWN name `rotate` rather than an invented
//! identifier. So for a `reify` fence, green means "the compiler accepted
//! this", NOT "every name in it resolves"; a reader can still pay a probe cycle
//! at eval time. The limitation is pinned executably by
//! `an_unresolved_call_name_compiles_clean_so_a_green_reify_tag_is_weaker`
//! below, so the day the compiler starts resolving call names that test goes
//! red and this paragraph is deleted rather than quietly rotting into a false
//! disclaimer.
//!
//! The corpus had exactly one instance of that gap and it is now GONE, which is
//! worth recording because the fix direction matters. `traits.md`'s `## Syntax`
//! fence carries the bare ```` ```reify ```` tag — the strongest claim in the
//! vocabulary — while binding `compute_moi(geometry, material.density)`, a
//! helper the fence's own inline comment disclaimed as "not a compiler/stdlib
//! function". Attaching the strongest truth claim to a body containing a known
//! phantom moved against this task's own grain, and DOCUMENTING why was the
//! wrong resolution: the body was made honest instead, rewritten as the
//! stdlib's own `trait Rigid` (`stdlib/structural_physical.ri`) over the real
//! `moment_of_inertia` builtin, keeping the tag and the traits floor at 3. The
//! now-obsolete `pdoccover:allow — placeholder` marker went with it (that lane
//! reported `compute_moi` as a `fabricated-name`; with the phantom gone the
//! suppression is dead weight). The general limitation above still stands — it
//! is a property of the compiler, not of that one fence — which is why its
//! pinning test is a synthetic fixture rather than a reference to a chunk that
//! can be fixed out from under it.

use reify_test_support::{compile_source_with_stdlib_allow_parse_errors, errors_only};

use crate::geometry_chunk_smoke::reify_tagged_fences;

// ---------------------------------------------------------------------------
// Fence parser
// ---------------------------------------------------------------------------

/// One fenced code block, as this gate sees it.
#[derive(Debug, Clone)]
struct Fence {
    /// 1-based position in document order across the whole file. This, not the
    /// line number, is what a violation message leads with: a reader counting
    /// fences down a rendered chunk can find "fence #4" without a line-numbered
    /// view of the source.
    ordinal: usize,
    /// 1-based line number of the OPENING delimiter.
    open_line: usize,
    /// The info string with surrounding whitespace trimmed; `None` for a bare
    /// opening delimiter.
    tag: Option<String>,
    /// Fence content, excluding BOTH delimiter lines.
    body: String,
}

/// Parse every fenced code block in `content`, in document order.
///
/// A hand-rolled line-level state machine rather than a markdown crate: the
/// gate needs the OPENING line number and the raw info string of each block,
/// and it must apply a stricter delimiter rule than CommonMark (below). Pulling
/// in a markdown dependency for a test-only scan of 17 files would buy neither.
///
/// A delimiter is a run of THREE OR MORE of a single FENCE CHARACTER —
/// backtick or tilde, CommonMark allows both — starting at column 0. The run is
/// COUNTED, not assumed to be exactly three, and the raw line is deliberately
/// NOT `trim_start`-ed, so anything indented is body content. That is stricter
/// than `enums_chunk_option_smoke.rs:106`'s `trim_start().strip_prefix("```")`
/// on the indentation axis and faithful to CommonMark on the run-length and
/// fence-character axes — and all three matter for one reason: a misread
/// delimiter inverts the open/close state for the entire rest of the file,
/// silently mislabelling every subsequent fence.
///
/// # Why the fence CHARACTER is tracked, not just backticks
///
/// No chunk uses `~~~` today, so this is latent rather than live — but the ban
/// this module enforces is advertised over EVERY fence, and a backtick-only
/// scan is blind to a tilde one on both axes. An untagged `~~~` would escape
/// `no_chunk_fence_is_untagged` silently, which is the ban quietly not
/// applying. Worse, a `~~~text` block whose body contains a column-0
/// ```` ```reify ```` line — the natural way to write a chunk that DOCUMENTS
/// this gate's vocabulary — would be read as a genuine open `reify` fence,
/// desyncing the scan for the rest of the file in exactly the way the run-length
/// counting below exists to prevent. A closer must therefore match BOTH the
/// opener's character and at least its length; a run of the other character is
/// ordinary body content, whatever its length.
///
/// # Why the run length is counted rather than assumed
///
/// CommonMark requires a CLOSING delimiter to be a run of the OPENER'S OWN
/// character, at least as long as the opening one. That rule is what lets a
/// markdown file NEST a fence — and the obvious future chunk to do so is one
/// documenting this gate's own tag vocabulary, which needs a four-backtick
/// block wrapping a three-backtick ```` ```reify ```` sample. Under a plain
/// `strip_prefix("```")` scan that opener parses with a BACKTICK captured into
/// its info string (tag `` `text `` rather than `text`) and the first INNER
/// three-backtick line closes the block, after which every fence in the file
/// is off by one and `no_chunk_fence_is_untagged` reports a bare fence at a
/// line the author never wrote one on. Counting the run makes the shorter
/// inner lines ordinary body content, which is what they are.
///
/// # A delimiter carrying an info string while a fence is OPEN is an `Err`
///
/// CommonMark says a closing fence carries no info string, so a tagged
/// delimiter appearing inside an already-open fence of the same run length is,
/// strictly, body content. In a hand-maintained doc corpus it is almost always
/// a MISSING closer instead. Both readings silently mislabel the rest of the
/// file, and this gate exists to catch exactly that class of drift, so the
/// parser refuses to guess: it returns `Err` naming both lines and lets a
/// human decide which one they meant.
///
/// Open/close state is what makes the bare-fence ban possible at all. In
/// markdown a CLOSING delimiter is bare by syntax, so a stateless scan for a
/// column-0 bare ``` would flag every well-formed fence in the corpus.
///
/// Returns `Err` if a fence is still open at EOF, naming its opening line.
/// Silently dropping it would be the worst outcome for an omission-drift gate:
/// the offending block would vanish from the scan and the corpus test would go
/// green *because* the file is malformed.
fn parse_fences(content: &str) -> Result<Vec<Fence>, String> {
    /// The leading run of a single CommonMark fence character at column 0:
    /// `(character, length)`, or `None` for a line that starts with neither.
    ///
    /// Both characters are ASCII, so the returned length is a valid byte AND
    /// char boundary and the caller can slice the info string off with it.
    fn delimiter_run(line: &str) -> Option<(u8, usize)> {
        let first = line.as_bytes().first().copied()?;
        if first != b'`' && first != b'~' {
            return None;
        }
        let run = line.bytes().take_while(|byte| *byte == first).count();
        Some((first, run))
    }

    /// The fence currently open, if any.
    ///
    /// A named struct rather than a tuple: five positional fields read as
    /// noise at every destructuring site, and the two `usize`s (a LINE and a
    /// RUN LENGTH) are trivially swappable by accident.
    struct Open<'a> {
        line: usize,
        /// The opener's fence character. A closer must match it — this is what
        /// keeps a ```` ``` ```` line inside a `~~~` block from closing it.
        fence_char: u8,
        /// The opener's run length. A closer must be at least this long — this
        /// is what lets a longer outer fence nest a shorter inner one.
        run: usize,
        tag: Option<String>,
        body: Vec<&'a str>,
    }

    fn name_of(fence_char: u8) -> &'static str {
        if fence_char == b'~' { "tilde" } else { "backtick" }
    }

    let mut fences: Vec<Fence> = Vec::new();
    let mut open: Option<Open<'_>> = None;

    for (index, line) in content.lines().enumerate() {
        let line_no = index + 1;
        let delimiter = delimiter_run(line);

        // Does this line close the fence currently open? Only a run of the
        // OPENER'S OWN character, at least as long as the opener's. A shorter
        // run — or a run of the other character, at any length — is body
        // content, which is what makes a nested fence parse correctly and what
        // keeps a ```` ```reify ```` line inside a `~~~` block from being read
        // as a genuine open `reify` fence.
        let closes = match (&open, delimiter) {
            (Some(state), Some((char_here, run))) => {
                char_here == state.fence_char && run >= state.run
            }
            _ => false,
        };

        if closes {
            let (_, run) = delimiter.expect("closes implies a delimiter");
            let rest = &line[run..];
            let state = open.as_ref().expect("closes implies open");
            if !rest.trim().is_empty() {
                return Err(format!(
                    "code fence delimiter at line {line_no} carries an info string \
                     ({info}) while the fence opened at line {open_line} (run of \
                     {open_run} {kind}s) is still OPEN. A closing delimiter must be \
                     bare, so this is either a MISSING closer above or a nested fence \
                     that needs a longer outer run; either way, guessing would \
                     mislabel every fence after it",
                    info = rest.trim(),
                    open_line = state.line,
                    open_run = state.run,
                    kind = name_of(state.fence_char)
                ));
            }
            let state = open.take().expect("closes implies open");
            fences.push(Fence {
                ordinal: fences.len() + 1,
                open_line: state.line,
                tag: state.tag,
                body: state.body.join("\n"),
            });
            continue;
        }

        match open.as_mut() {
            // Anything that did not close the open fence is its body — including
            // a run SHORTER than the opener's, and a run of the OTHER fence
            // character at any length.
            Some(state) => state.body.push(line),
            // Outside any fence, a run of >= 3 opens one; an empty info string
            // is the untagged case the bare-fence ban reports.
            None => {
                if let Some((fence_char, run)) = delimiter.filter(|(_, run)| *run >= 3) {
                    let info = line[run..].trim();
                    open = Some(Open {
                        line: line_no,
                        fence_char,
                        run,
                        tag: (!info.is_empty()).then(|| info.to_string()),
                        body: Vec::new(),
                    });
                }
            }
        }
    }

    if let Some(state) = open {
        return Err(format!(
            "unterminated code fence: the delimiter opened at line {open_line} \
             (run of {open_run} {kind}s, info string {}) is never closed, so \
             every fence after it would be mislabelled — the scan cannot be \
             trusted",
            state.tag.as_deref().unwrap_or("<none>"),
            open_line = state.line,
            open_run = state.run,
            kind = name_of(state.fence_char)
        ));
    }

    Ok(fences)
}

// ---------------------------------------------------------------------------
// Check 2 — the bare-fence ban
// ---------------------------------------------------------------------------

/// Every fence in `content` that carries no language tag, one message each, in
/// document order.
///
/// Deliberately does NOT validate the tag against an allow-list — any explicit
/// tag exempts, per the OPEN vocabulary argued in the module header. The point
/// is not to police notation but to force the doc author to make a CLAIM about
/// the fence, which then shows up as a one-line diff a reviewer can challenge.
///
/// Takes ALREADY-PARSED fences: every caller reaches this through
/// `check_parse_outcome`, which owns the single copy of the parse-failure
/// handling. Re-parsing here would mean the anti-vacuity floors counted one
/// parse while the checks reported on a different one.
fn untagged_fence_violations(path: &str, fences: &[Fence]) -> Vec<String> {
    fences
        .iter()
        .filter(|fence| fence.tag.is_none())
        .map(|fence| {
            format!(
                "{path}:{} — fence #{} has NO language tag. Every fence must \
                 carry an explicit one: `reify` ONLY if the body compiles \
                 STANDALONE as a complete module; else `reify-fragment` (real \
                 reify syntax that is member-level or context-dependent), \
                 `reify-schematic` (not reify source at all — signature \
                 listing, metavariable notation, `{{ ... }}` elision), \
                 `reify-invalid` (a deliberate-error teaching sample), or the \
                 language it actually is (`ebnf`, `text`, …).",
                fence.open_line, fence.ordinal
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Compiling a fence body — ONE owner, for checks 1 and 3
// ---------------------------------------------------------------------------

/// What the compiler made of a fence body, split by the LAYER that rejected it.
///
/// `errors_only` alone cannot make this distinction: parse errors are folded
/// into the same `.diagnostics` list as compile-layer ones, so "did not parse"
/// and "parsed and then failed type checking" arrive indistinguishable. Check 3
/// needs them apart — see [`FenceCompile::ParseRejected`].
enum FenceCompile {
    /// Parsed, compiled, zero `Severity::Error` diagnostics.
    Clean,
    /// The PARSER rejected the body, so it is not reify source at all. Any
    /// compile-layer diagnostics downstream of a broken AST describe the
    /// wreckage rather than the body, which is why this arm carries only the
    /// parse messages.
    ParseRejected(Vec<String>),
    /// Parsed cleanly, then produced at least one `Severity::Error`.
    SemanticErrors(Vec<String>),
}

impl FenceCompile {
    /// The rendered diagnostics, one indented bullet per line.
    fn rendered(messages: &[String]) -> String {
        messages
            .iter()
            .map(|message| format!("    - {message}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Compile one fence body VERBATIM — no wrapper — and report which layer, if
/// any, rejected it.
///
/// # Why `_allow_parse_errors`
///
/// `compile_source_with_stdlib` (`helpers.rs:236`) PANICS on parse errors. One
/// malformed fence would then abort the whole gate with a backtrace naming no
/// file and no fence — defeating the "names file + fence ordinal +
/// diagnostics" contract at exactly the moment it matters most. The
/// `_allow_parse_errors` variant (`helpers.rs:354`) folds parse errors into
/// `.diagnostics` at Error severity via `parse_errors_as_diagnostics`, so a
/// malformed fence is reported as a normal, fully-attributed violation. Same
/// accumulate-rather-than-panic reasoning `examples_smoke.rs` applies in its
/// parse phase.
///
/// The extra `parse_with_stdlib` call is what separates the two layers. It is
/// the SAME parse the helper performs internally, repeated rather than
/// threaded out, because the helper's signature returns only a
/// `CompiledModule`; a string match on the diagnostic text would be the
/// alternative, and an ad-hoc parser over a message is what heuristic 12 exists
/// to forbid.
///
/// The body is compiled VERBATIM — no wrapper. That is what makes bare
/// ```` ```reify ```` mean "compiles standalone" rather than "compiles under
/// whatever scaffolding some harness happens to inject".
fn compile_fence_body(body: &str) -> FenceCompile {
    let parsed = reify_compiler::parse_with_stdlib(body, reify_core::ModulePath::single("fence"));
    if !parsed.errors.is_empty() {
        return FenceCompile::ParseRejected(
            parsed.errors.iter().map(|e| e.message.clone()).collect(),
        );
    }
    let compiled = compile_source_with_stdlib_allow_parse_errors(body);
    let errors = errors_only(&compiled);
    if errors.is_empty() {
        FenceCompile::Clean
    } else {
        FenceCompile::SemanticErrors(
            errors
                .iter()
                .map(|diagnostic| diagnostic.message.clone())
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// Check 1 — a bare ```reify fence must compile standalone
// ---------------------------------------------------------------------------

/// Every fence tagged EXACTLY ```` ```reify ```` whose body does not compile
/// as a complete module with zero `Severity::Error` diagnostics.
///
/// # EXACT tag match, never a prefix
///
/// The filter is `tag.as_deref() == Some("reify")`. `starts_with("reify")`
/// would sweep in `reify-fragment` / `reify-schematic` / `reify-invalid` and
/// trial-compile the entire exempt half of the corpus, which is precisely what
/// those tags exist to prevent.
///
/// Both rejection layers are violations here — see [`compile_fence_body`],
/// which owns the compile and the layer split.
///
/// Takes ALREADY-PARSED fences, for the reason given on
/// `untagged_fence_violations`: `check_parse_outcome` is the one place a parse
/// failure is turned into a violation.
fn reify_fence_violations(path: &str, fences: &[Fence]) -> Vec<String> {
    fences
        .iter()
        .filter(|fence| fence.tag.as_deref() == Some("reify"))
        .filter_map(|fence| {
            let messages = match compile_fence_body(&fence.body) {
                FenceCompile::Clean => return None,
                FenceCompile::ParseRejected(messages) | FenceCompile::SemanticErrors(messages) => {
                    messages
                }
            };
            Some(format!(
                "{path}:{} — fence #{} is tagged ```reify but does NOT compile \
                 standalone; {} Error diagnostic(s):\n{}\n  --- fence \
                 body ---\n{}\n  --- end fence body ---\n  Either fix the body, \
                 or retag: `reify-fragment` if it is real reify syntax needing \
                 context it cannot carry, `reify-schematic` if it is not reify \
                 source at all, `reify-invalid` if the error is the lesson.",
                fence.open_line,
                fence.ordinal,
                messages.len(),
                FenceCompile::rendered(&messages),
                fence.body
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Check 3 — a ```reify-invalid fence must ACTUALLY be invalid
// ---------------------------------------------------------------------------

/// Every fence tagged EXACTLY ```` ```reify-invalid ```` that does not actually
/// demonstrate what the tag claims. TWO ways to fail it, because the tag makes
/// a claim about SEMANTICS — this body is reify, and the compiler's verdict on
/// it is the lesson:
///
/// - a CLEAN compile, the obvious one: the sample no longer errors at all.
/// - a PARSE rejection, the subtle one: the body never reached the phase whose
///   verdict was being taught, so the tag is satisfied by "this is not reify"
///   rather than by the documented error. Without this arm the tag is
///   vacuously green under arbitrary prose, and every sample's real lesson can
///   rot away unnoticed behind a syntax error introduced anywhere above it.
///
/// Why this tag and not the other two exempt ones is argued once, in the module
/// header's tag vocabulary.
///
/// Takes ALREADY-PARSED fences, for the reason given on
/// `untagged_fence_violations`.
fn reify_invalid_fence_violations(path: &str, fences: &[Fence]) -> Vec<String> {
    fences
        .iter()
        .filter(|fence| fence.tag.as_deref() == Some("reify-invalid"))
        .filter_map(|fence| {
            let failure = match compile_fence_body(&fence.body) {
                FenceCompile::SemanticErrors(_) => return None,
                FenceCompile::Clean => "compiles CLEAN: zero Error diagnostics. \
                     That tag asserts the error IS the lesson, so either the \
                     teaching sample no longer demonstrates what it claims (the \
                     compiler changed, or the body drifted), or the tag is being \
                     used to silence a fence that should be fixed and retagged \
                     `reify`."
                    .to_string(),
                FenceCompile::ParseRejected(messages) => format!(
                    "does not PARSE, so the compiler never reached the phase \
                     whose verdict this sample teaches; its {} diagnostic(s) \
                     are the parser's, and an unparseable body would satisfy \
                     this tag just as well as arbitrary prose:\n{}\n  Give the \
                     sample whatever context it needs to parse as a complete \
                     module — then the documented error is what the compiler \
                     actually reports. If the rejection really is a SYNTAX \
                     lesson, it belongs under `reify-schematic` with the \
                     rejection spelled out in prose.",
                    messages.len(),
                    FenceCompile::rendered(&messages)
                ),
            };
            Some(format!(
                "{path}:{} — fence #{} is tagged ```reify-invalid but {failure}\
                 \n  --- fence body ---\n{}\n  --- end fence body ---",
                fence.open_line, fence.ordinal, fence.body
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Parse-failure handling — ONE owner
//
// A malformed chunk must never abort the run. `corpus()` therefore carries each
// file's parse OUTCOME rather than unwrapping it, and every check reaches its
// fences through `check_parse_outcome`, which is the single place a parse
// failure becomes a violation string. Two chunks that each acquire an
// unterminated fence are then both reported by one run — the
// one-fix-cycle-not-N property this module's header claims.
// ---------------------------------------------------------------------------

/// A per-file check over ALREADY-PARSED fences.
type FenceCheck = fn(&str, &[Fence]) -> Vec<String>;

/// Apply `check` to one file's parse outcome, folding a parse failure into the
/// same accumulated violation list as everything else.
fn check_parse_outcome(
    path: &str,
    parsed: &Result<Vec<Fence>, String>,
    check: FenceCheck,
) -> Vec<String> {
    match parsed {
        Ok(fences) => check(path, fences),
        Err(error) => vec![format!("{path}: {error}")],
    }
}

/// Parse `content` and run `check` over it — the shape the hermetic tests use,
/// going through exactly the same fold as the real corpus so the parse-failure
/// path below is genuinely exercised rather than merely present.
fn check_markdown(path: &str, content: &str, check: FenceCheck) -> Vec<String> {
    check_parse_outcome(path, &parse_fences(content), check)
}

// ---------------------------------------------------------------------------
// Corpus discovery
//
// reify-mcp does NOT depend on reify-compiler, so these files cannot be
// `include_str!`-ed from here — they are read by path via the
// `CARGO_MANIFEST_DIR` idiom that
// `harness_compilation_surface/examples_smoke.rs`'s `EXAMPLES_DIR` (:15) and
// `geometry_chunk_smoke.rs`'s `CHUNK_PATH` (:351) already use. A wrong path
// fails loudly at read time rather than silently scanning nothing.
// ---------------------------------------------------------------------------

const CHUNKS_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/chunks"
);

const LANGUAGE_CHUNKS_RS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reify-mcp/src/tools/language_chunks.rs"
);

/// Every `*.md` stem in the chunk dir, PATH-SORTED.
///
/// Sorted because `read_dir` order is filesystem-dependent: without this a
/// failure list would shuffle between machines and a diff of two runs would be
/// unreadable. Mirrors `pdoccover`'s sorted-corpus discipline.
fn discover_chunk_stems() -> Vec<String> {
    let entries = std::fs::read_dir(CHUNKS_DIR).unwrap_or_else(|e| {
        panic!("{CHUNKS_DIR} must be readable ({e}) — update CHUNKS_DIR if the chunk dir moved")
    });

    let mut stems: Vec<String> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("{CHUNKS_DIR}: unreadable dir entry ({e})"))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter_map(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();
    stems.sort();
    stems
}

/// The text of one chunk file.
fn read_chunk_file(stem: &str) -> String {
    let path = format!("{CHUNKS_DIR}/{stem}.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path} must be readable ({e})"))
}

/// The repo-relative label used in violation messages, so a failure reads as a
/// path a developer can open rather than an absolute build-machine path.
fn chunk_label(stem: &str) -> String {
    format!("crates/reify-mcp/src/tools/chunks/{stem}.md")
}

// ---------------------------------------------------------------------------
// Check 4 — every chunk is reachable through the MCP tool
// ---------------------------------------------------------------------------

/// The exact prefix of the `TOPICS` slice literal in `language_chunks.rs`.
const TOPICS_ANCHOR: &str = "pub const TOPICS: &[&str] = &[";

/// The body of the `TOPICS` slice literal, or `None` if it cannot be located.
fn topics_literal(src: &str) -> Option<&str> {
    let start = src.find(TOPICS_ANCHOR)? + TOPICS_ANCHOR.len();
    let rest = &src[start..];
    let end = rest.find("];")?;
    Some(&rest[..end])
}

/// Every chunk stem on disk that is not reachable through the
/// `reify_language_reference` MCP tool.
///
/// Reachability needs BOTH halves and they fail differently:
///
/// - no `include_str!("chunks/<stem>.md")` — the file is not compiled into the
///   binary at all. Whole-file omission drift: it ships in the repo and is
///   served to nobody.
/// - not in the `TOPICS` slice literal — subtler, and the reason the TOPICS
///   half is scoped to that literal rather than scanned file-wide. Such a
///   chunk compiles in and even answers `get_chunk`, but `TOPICS` is what the
///   MCP tool ENUMERATES, so no caller can discover it. A whole-file scan
///   would be satisfied by the `"<stem>" => Some(CONST)` match arm and miss
///   this entirely.
///
/// Both scans are ANCHORED — `include_str!("chunks/<stem>.md")` in full, and
/// the QUOTED `"<stem>"` for the topic entry — so a stem can never be satisfied
/// by a coincidental substring of a longer one (`types` inside `prototypes`).
/// An unanchored scan would report the corpus clean while a chunk was served to
/// nobody, which is the exact silent failure this check exists to prevent.
fn reachability_violations(stems: &[String], src: &str) -> Vec<String> {
    let Some(topics) = topics_literal(src) else {
        return vec![format!(
            "could not locate the `{TOPICS_ANCHOR}` slice literal in \
             language_chunks.rs — the reachability scan is anchored on it, so a \
             move or rename must fail LOUDLY here rather than silently \
             reporting the whole corpus clean (or the whole corpus broken)"
        )];
    };

    stems
        .iter()
        .filter_map(|stem| {
            let include = format!("include_str!(\"chunks/{stem}.md\")");
            let topic_entry = format!("\"{stem}\"");

            let mut missing: Vec<String> = Vec::new();
            if !src.contains(&include) {
                missing.push(format!(
                    "no `{include}` in language_chunks.rs, so the file is not \
                     compiled into the binary at all"
                ));
            }
            if !topics.contains(&topic_entry) {
                missing.push(format!(
                    "no `{topic_entry}` entry in the `TOPICS` slice literal, so \
                     the `reify_language_reference` MCP tool cannot enumerate it \
                     even if it compiles in"
                ));
            }
            if missing.is_empty() {
                return None;
            }

            Some(format!(
                "{} is on disk but UNREACHABLE: {}",
                chunk_label(stem),
                missing.join("; and ")
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Hermetic parser tests
//
// Every case below runs on SYNTHETIC in-memory markdown. No chunk file on disk
// is read or mutated, so the parser's own contract is pinned independently of
// whatever the real corpus happens to contain today.
// ---------------------------------------------------------------------------

/// A bare ``` opening delimiter yields `tag == None`, and its bare closing
/// delimiter is consumed as a delimiter rather than mistaken for a second
/// untagged opening.
///
/// This open/close state discrimination is the whole reason the gate cannot be
/// a `grep`: in markdown a CLOSING delimiter is bare by syntax, so a stateless
/// scan for `^```$` would flag every well-tagged fence in the corpus.
#[test]
fn bare_opening_fence_parses_untagged_and_its_closer_is_not_a_second_block() {
    let md = "intro prose\n\
              ```\n\
              bare body\n\
              ```\n\
              trailing prose\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        1,
        "the closing ``` must be consumed as a delimiter, not parsed as a \
         second untagged block; got {fences:#?}"
    );
    assert_eq!(fences[0].ordinal, 1, "ordinals are 1-based");
    assert_eq!(fences[0].open_line, 2, "open_line is 1-based");
    assert_eq!(fences[0].tag, None, "a bare opening delimiter carries no tag");
    assert_eq!(
        fences[0].body, "bare body",
        "body excludes BOTH delimiter lines"
    );
}

/// The bare closing delimiter of a TAGGED fence never surfaces as an untagged
/// block. Stated separately from the case above because this is the shape the
/// bare-fence ban must not false-positive on: every compliant fence in the
/// corpus ends with a bare ```.
#[test]
fn the_bare_closer_of_a_tagged_fence_is_not_reported_as_untagged() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
    assert!(
        fences.iter().all(|f| f.tag.is_some()),
        "the closing delimiter must not appear as an untagged fence"
    );
}

/// EXACT tag semantics: `reify-fragment` is its own tag and must never be read
/// as a bare `reify` by prefix matching.
///
/// This is the single most load-bearing parser property. `reify_fence_violations`
/// selects on `tag.as_deref() == Some("reify")`; if the tag were captured (or
/// compared) by prefix, every `reify-fragment` / `reify-schematic` fence in the
/// corpus would be trial-compiled, and the whole exempt-tag vocabulary would
/// collapse.
#[test]
fn hyphenated_tags_are_exact_and_never_collapse_to_bare_reify() {
    for tag in ["reify-fragment", "reify-schematic", "reify-invalid"] {
        let md = format!("```{tag}\nlet x = 1mm\n```\n");
        let fences = parse_fences(&md).expect("well-formed markdown must parse");

        assert_eq!(fences.len(), 1, "tag `{tag}`: got {fences:#?}");
        assert_eq!(
            fences[0].tag.as_deref(),
            Some(tag),
            "tag `{tag}` must be captured verbatim"
        );
        assert_ne!(
            fences[0].tag.as_deref(),
            Some("reify"),
            "tag `{tag}` must NOT be readable as bare `reify` — prefix matching \
             here would trial-compile every exempt fence in the corpus"
        );
    }
}

/// Trailing whitespace after an info string is trimmed, so a fence tagged
/// `` ```reify `` with a stray trailing space is still exactly `reify`.
#[test]
fn trailing_whitespace_after_a_tag_is_trimmed() {
    let md = "```reify   \nstructure def S { let n = 1 }\n```\n";
    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
}

/// A delimiter-looking line that is INDENTED is body content, not a delimiter.
///
/// The gate's rule is "fences at line start" — deliberately stricter than
/// `enums_chunk_option_smoke.rs`'s `line.trim_start().strip_prefix("```")`, so
/// an indented ``` inside a body cannot silently close the block and desync the
/// parser for the whole rest of the file.
#[test]
fn an_indented_delimiter_is_body_content_not_a_delimiter() {
    let md = "```text\n\
              outer\n\
              \x20   ```\n\
              still outer\n\
              ```\n\
              after\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        1,
        "the indented ``` must not open or close a block; got {fences:#?}"
    );
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
    assert_eq!(fences[0].body, "outer\n    ```\nstill outer");
}

/// A FOUR-backtick fence nests a three-backtick sample as body content.
///
/// This is the CommonMark way to show a fenced block inside a fenced block —
/// exactly what a future chunk documenting this gate's own tag vocabulary
/// would need. A naive `strip_prefix("```")` scan mis-parses it twice over:
/// the opener's tag becomes `` `text `` (a backtick swallowed into the info
/// string) and the first INNER delimiter closes the block, leaving every
/// later fence in the file off by one and the bare-fence ban reporting a
/// violation at a line the author never wrote a bare fence on.
#[test]
fn a_four_backtick_fence_nests_a_three_backtick_sample_as_body() {
    let md = "````text\n\
              ```reify\n\
              structure def S { let n = 1 }\n\
              ```\n\
              ````\n\
              after\n";

    let fences = parse_fences(md).expect("a nested fence is well-formed markdown");

    assert_eq!(
        fences.len(),
        1,
        "the inner three-backtick lines are BODY of the four-backtick block, not \
         delimiters of their own; got {fences:#?}"
    );
    assert_eq!(
        fences[0].tag.as_deref(),
        Some("text"),
        "the info string is what follows the COUNTED run; a `strip_prefix(\"```\")` \
         scan would report the tag as `` `text `` and no exact-match check would \
         ever recognise it again"
    );
    assert_eq!(
        fences[0].body, "```reify\nstructure def S { let n = 1 }\n```",
        "both inner delimiter lines belong to the body verbatim"
    );
}

/// A closing run LONGER than the opening one still closes it (CommonMark: the
/// closer must be *at least* as long, not exactly as long).
/// A `~~~` fence is a delimiter too, so an untagged one does not escape the ban.
///
/// CommonMark allows tilde fences everywhere backtick fences are allowed, and
/// this module's header advertises the ban over EVERY fence. A backtick-only
/// scan would leave `~~~` as a silent hole in exactly the check whose whole
/// value is that it has none.
#[test]
fn an_untagged_tilde_fence_is_a_delimiter_and_is_reported() {
    let md = "prose\n\
              ~~~\n\
              bare body\n\
              ~~~\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");
    assert_eq!(
        fences.len(),
        1,
        "a `~~~` pair is ONE fence, not zero and not two; got {fences:#?}"
    );
    assert_eq!(fences[0].tag, None);
    assert_eq!(fences[0].open_line, 2);
    assert_eq!(fences[0].body, "bare body");

    let violations = check_markdown("chunks/x.md", md, untagged_fence_violations);
    assert_eq!(
        violations.len(),
        1,
        "an untagged tilde fence must be reported like any other, got {violations:#?}"
    );
}

/// A tagged `~~~` fence carries its info string, and its bare closer is not a
/// second block — the tilde mirror of the backtick contract above.
#[test]
fn a_tagged_tilde_fence_carries_its_info_string() {
    let md = "~~~text\n\
              plain prose sample\n\
              ~~~\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");
    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
}

/// THE DESYNC CASE. A column-0 ```` ```reify ```` line inside a `~~~` block is
/// BODY, never a fence.
///
/// This is the failure mode a backtick-only parser cannot see and the reason
/// the fence CHARACTER is tracked rather than assumed. Under a backtick-only
/// scan the inner line opens a `reify` fence the author never wrote, the outer
/// `~~~` closer is not a backtick run so the block never closes, and the parse
/// either errors at EOF or mislabels every fence after it — while
/// `every_reify_tagged_fence_compiles_clean` tries to compile a body that is
/// really a chunk of prose. The one shape most likely to hit this is a chunk
/// documenting THIS gate's tag vocabulary, which needs to show a ```` ```reify ````
/// line without it being one.
#[test]
fn a_backtick_fence_line_inside_a_tilde_block_is_body_not_a_fence() {
    let md = "~~~text\n\
              ```reify\n\
              structure def NotReallyAFence { let n = 1 }\n\
              ```\n\
              ~~~\n\
              ```reify\n\
              structure def GenuinelyAFence { let n = 1 }\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        2,
        "the tilde block is ONE fence and the trailing backtick block is the \
         other — the inner ```reify line is body. Got {fences:#?}"
    );
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
    assert!(
        fences[0].body.contains("```reify"),
        "the inner delimiter line must survive INTO the body verbatim, got: {}",
        fences[0].body
    );
    assert_eq!(
        fences[1].tag.as_deref(),
        Some("reify"),
        "the scan must still be in sync after the tilde block — a desync here \
         mislabels every fence in the rest of the file"
    );
    assert_eq!(
        fences[1].open_line, 6,
        "and the open line of the genuine fence must be exact, got {fences:#?}"
    );
}

/// A `~~~` run never closes a backtick fence, whatever its length.
///
/// The converse of the case above, and the property that makes the closer rule
/// symmetric: mismatching characters are body content in both directions.
#[test]
fn a_tilde_run_does_not_close_a_backtick_fence() {
    let md = "```text\n\
              ~~~~~~\n\
              still inside the backtick fence\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");
    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert!(
        fences[0].body.contains("~~~~~~"),
        "the long tilde run is body content, got: {}",
        fences[0].body
    );
}

#[test]
fn a_closing_run_longer_than_the_opening_one_still_closes_it() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ````\n";

    let fences = parse_fences(md).expect("a longer closing run is well-formed");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
    assert_eq!(fences[0].body, "structure def S { let n = 1 }");
}

/// A TAGGED delimiter appearing while a fence of the same run length is still
/// open is a named `Err`, not a silent guess.
///
/// CommonMark would read it as body; a hand-maintained corpus almost always
/// means a missing closer on the line above. Both readings mislabel every
/// fence after it, which is the precise drift this gate exists to catch, so
/// the parser refuses and names BOTH lines.
#[test]
fn a_tagged_delimiter_inside_an_open_fence_is_a_named_error() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```reify-fragment\n\
              let n = 1\n\
              ```\n";

    let err = parse_fences(md).expect_err("an info string on a closer must not parse clean");

    assert!(
        err.contains('1') && err.contains('3'),
        "the error must name BOTH the open line (1) and the offending delimiter \
         line (3), got: {err}"
    );
    assert!(
        err.contains("reify-fragment"),
        "the error must quote the info string that made the line ambiguous, got: {err}"
    );
}

/// Ordinals are assigned in document order across the whole file, and each
/// fence records its own 1-based opening line.
#[test]
fn ordinals_and_open_lines_follow_document_order() {
    let md = "# Title\n\
              ```reify\n\
              a\n\
              ```\n\
              prose\n\
              ```\n\
              b\n\
              ```\n\
              ```reify-fragment\n\
              c\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    let seen: Vec<(usize, usize, Option<&str>)> = fences
        .iter()
        .map(|f| (f.ordinal, f.open_line, f.tag.as_deref()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (1, 2, Some("reify")),
            (2, 6, None),
            (3, 9, Some("reify-fragment")),
        ],
        "ordinals must be 1-based and in document order, open_line 1-based"
    );
}

/// An unterminated final fence is an `Err` naming the opening line — never a
/// silently dropped block.
///
/// Dropping it would be the worst possible failure mode for an omission-drift
/// gate: the offending fence would vanish from the scan and the corpus test
/// would go green precisely because the file is malformed.
#[test]
fn an_unterminated_final_fence_is_an_error_naming_its_opening_line() {
    let md = "prose\n\
              ```reify\n\
              structure def S { let n = 1 }\n";

    let err = parse_fences(md).expect_err("an unterminated fence must not parse clean");

    assert!(
        err.contains('2'),
        "the error must name the OPENING line (2) of the unterminated fence, got: {err}"
    );
    assert!(
        err.to_lowercase().contains("unterminated"),
        "the error must say what went wrong, got: {err}"
    );
}

/// An empty fence body is `""`, not a parse failure.
#[test]
fn an_empty_fence_body_parses_as_the_empty_string() {
    let md = "```reify-schematic\n```\n";
    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].body, "");
}

// ---------------------------------------------------------------------------
// Check 2 — the bare-fence ban
// ---------------------------------------------------------------------------

/// An untagged OPENING fence is one violation, and the message carries both the
/// file path and `:<line>` of the opening delimiter.
///
/// `file:line` specifically, not just a count: the whole value of this check is
/// that a doc author who trips it can jump straight to the offending line. A
/// bare "3 untagged fences" would send them re-counting delimiters by hand.
#[test]
fn an_untagged_opening_fence_is_reported_with_file_and_line() {
    let md = "# Collections\n\
              \n\
              ```\n\
              let xs = [1, 2, 3]\n\
              ```\n";

    let violations = check_markdown("chunks/collections.md", md, untagged_fence_violations);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/collections.md"),
        "the violation must name the file, got: {}",
        violations[0]
    );
    assert!(
        violations[0].contains(":3"),
        "the violation must name `:<opening line>` (here `:3`), got: {}",
        violations[0]
    );
}

/// The bare CLOSING delimiter of a tagged fence is never a violation.
///
/// This is the false positive that would make the check unusable: in markdown
/// every fence closes with a bare ```, so a stateless scan would report a
/// violation for every single compliant fence in the corpus. Only the parser's
/// open/close state distinguishes them.
#[test]
fn the_bare_closing_delimiter_of_a_tagged_fence_is_not_a_violation() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```\n";

    assert!(
        check_markdown("chunks/whatever.md", md, untagged_fence_violations).is_empty(),
        "a compliant tagged fence closes with a bare ``` and must stay clean"
    );
}

/// ANY explicit tag exempts — the OPEN vocabulary of the module header, pinned
/// executably, including on a tag no one here anticipated.
#[test]
fn every_explicit_tag_exempts_including_ones_this_task_never_anticipated() {
    for tag in [
        "reify",
        "reify-fragment",
        "reify-schematic",
        "reify-invalid",
        "text",
        "ebnf",
        "json",
        "some-future-notation",
    ] {
        let md = format!("```{tag}\nbody\n```\n");
        assert!(
            check_markdown("chunks/x.md", &md, untagged_fence_violations).is_empty(),
            "tag `{tag}` is explicit and must exempt the fence — the allow-list \
             is open by design"
        );
    }
}

/// Several offending fences produce one violation each, in document order, so a
/// single run surfaces the whole backlog rather than one fence at a time.
#[test]
fn multiple_untagged_fences_are_each_reported_in_document_order() {
    let md = "```\n\
              first\n\
              ```\n\
              prose\n\
              ```reify-schematic\n\
              exempt\n\
              ```\n\
              more prose\n\
              ```\n\
              second\n\
              ```\n";

    let violations = check_markdown("chunks/units.md", md, untagged_fence_violations);

    assert_eq!(violations.len(), 2, "got {violations:#?}");
    assert!(
        violations[0].contains(":1"),
        "first violation must be the line-1 fence, got: {}",
        violations[0]
    );
    assert!(
        violations[1].contains(":9"),
        "second violation must be the line-9 fence, got: {}",
        violations[1]
    );
}

/// The remedy is spelled out in the message, so the fix does not require
/// reading this module first.
#[test]
fn the_violation_message_points_at_the_tag_vocabulary() {
    let violations = check_markdown("chunks/x.md", "```\nbody\n```\n", untagged_fence_violations);

    let message = &violations[0];
    for expected in ["reify-fragment", "reify-schematic"] {
        assert!(
            message.contains(expected),
            "the remedy must name `{expected}` so the fix needs no source dive, \
             got: {message}"
        );
    }
}

/// A file that does not PARSE is reported as a violation by BOTH checks —
/// never a panic, and never a silent zero.
///
/// This is the accumulate-then-report-all contract at its most load-bearing
/// point. `check_parse_outcome` is the only owner of this fold, and both the
/// hermetic tests here and the real corpus reach it, so the path is genuinely
/// exercised rather than merely written. Were the corpus to panic on the first
/// malformed file instead, two chunks that each acquired an unterminated fence
/// would take two fix cycles to discover: the run would name only the
/// alphabetically-first.
#[test]
fn a_file_that_does_not_parse_is_reported_by_both_checks_not_panicked_on() {
    let unterminated = "prose\n\
                        ```reify\n\
                        structure def S { let n = 1 }\n";

    for check in [
        untagged_fence_violations as FenceCheck,
        reify_fence_violations as FenceCheck,
    ] {
        let violations = check_markdown("chunks/broken.md", unterminated, check);

        assert_eq!(
            violations.len(),
            1,
            "a malformed file is exactly one violation — the parse failure \
             itself; got {violations:#?}"
        );
        assert!(
            violations[0].starts_with("chunks/broken.md"),
            "the violation must lead with the file, so a corpus run naming \
             several of them is readable; got: {}",
            violations[0]
        );
        assert!(
            violations[0].to_lowercase().contains("unterminated"),
            "the violation must carry the parser's own reason, got: {}",
            violations[0]
        );
    }
}

/// Two malformed files in ONE run produce TWO violations.
///
/// The property the fold exists for, asserted directly: a `panic!` on the first
/// parse failure would make this impossible and cost one fix cycle per
/// malformed file.
#[test]
fn several_malformed_files_are_all_reported_by_a_single_run() {
    const UNTERMINATED: &str = "```reify\nstructure def S { let n = 1 }\n";

    let docs: Vec<(&str, Result<Vec<Fence>, String>)> = ["chunks/a.md", "chunks/z.md"]
        .into_iter()
        .map(|label| (label, parse_fences(UNTERMINATED)))
        .collect();

    let violations: Vec<String> = docs
        .iter()
        .flat_map(|(label, parsed)| check_parse_outcome(label, parsed, untagged_fence_violations))
        .collect();

    assert_eq!(
        violations.len(),
        2,
        "both malformed files must appear in one run, not just the first; got \
         {violations:#?}"
    );
    assert!(
        violations[0].starts_with("chunks/a.md") && violations[1].starts_with("chunks/z.md"),
        "each violation names its own file, in corpus order; got {violations:#?}"
    );
}

// ---------------------------------------------------------------------------
// Check 1 — a bare ```reify fence must compile standalone
//
// THE RED-FIRST DEMONSTRATION. Every case runs against SYNTHETIC markdown, so
// the gate is proven to go red without ever mutating a shipped chunk file and
// without leaving a planted defect behind. The fixtures then survive as
// permanent regression tests rather than as a one-off manual demonstration
// that rots.
// ---------------------------------------------------------------------------

/// A ```` ```reify ````-tagged fence containing the PHANTOM 3-arg `rotate` is
/// reported, with the compiler's own words.
///
/// # Why THIS phantom
///
/// The demo must plant an ARITY error on a KNOWN name, not an invented
/// identifier. `geometry_chunk_smoke.rs`'s scope statement establishes that an
/// unknown call NAME is frequently NOT an error — a `structure def` body types
/// an unresolved call from its first argument's `result_type` — so a made-up
/// name would compile clean and this demo would silently prove nothing, leaving
/// a vacuous gate behind. `rotate` instead dispatches purely on arity at
/// `geometry_transform.rs:37` (2 or 5 args only); a 3-arg call reaches the
/// `n =>` arm and calls `push_labeled_arg_count_error` (`arg_check.rs:67`),
/// which builds a genuine `Severity::Error` `Diagnostic`.
///
/// This is not a hypothetical shape either: `functions.md`'s overloading
/// example shipped exactly this 3-arg `rotate` form until #6890 renamed it to
/// `align`. Finding it cost a printer_v01 probe cycle; this gate is what makes
/// the compiler say it first.
#[test]
fn a_reify_fence_whose_body_calls_the_phantom_three_arg_rotate_is_reported() {
    let md = "prose\n\
              ```reify\n\
              structure def PhantomRotate {\n\
              \x20   let blank = box(20mm, 20mm, 20mm)\n\
              \x20   let turned = rotate(blank, vec3(0.0, 0.0, 1.0), 45deg)\n\
              }\n\
              ```\n";

    let violations = check_markdown("chunks/functions.md", md, reify_fence_violations);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    let message = &violations[0];
    assert!(
        message.contains("chunks/functions.md"),
        "the violation must name the file, got: {message}"
    );
    assert!(
        message.contains("fence #1"),
        "the violation must name the fence ORDINAL, so a reader counting fences \
         down a rendered chunk can find it without a line-numbered view, got: \
         {message}"
    );
    assert!(
        message.contains(":2"),
        "the violation must name the fence's OPENING line (`:2`), got: {message}"
    );
    assert!(
        message.contains("rotate() expects 2 or 5 arguments, got 3"),
        "the violation must carry the COMPILER'S OWN diagnostic text — that is \
         the whole point: the gate replaces a printer_v01 probe cycle with the \
         compiler saying it directly. Got: {message}"
    );
}

/// A clean self-contained `structure def` fence produces no violation.
///
/// The control for the phantom above: without it, a `reify_fence_violations`
/// that flagged everything would pass the phantom test while being useless.
#[test]
fn a_clean_self_contained_reify_fence_is_not_reported() {
    let md = "```reify\n\
              structure def Clean {\n\
              \x20   let blank = box(20mm, 20mm, 20mm)\n\
              }\n\
              ```\n";

    assert!(
        check_markdown("chunks/x.md", md, reify_fence_violations).is_empty(),
        "a self-contained module that compiles clean must not be flagged"
    );
}

/// An UNRESOLVED CALL NAME compiles clean, so a green `reify` tag is weaker
/// than the tag's stated meaning reads.
///
/// The limitation named in this module's header, made executable. The fixture
/// is deliberately SYNTHETIC rather than a reference to a live chunk: the
/// corpus instance that motivated it — `traits.md`'s `compute_moi(...)`
/// placeholder — was fixed by making that body honest, and a test anchored to a
/// chunk would have been deleted along with it, taking the compiler property it
/// pins with it. A body whose only call is a name that exists nowhere still
/// produces zero Error diagnostics.
///
/// If this test ever FAILS, that is good news, not a regression: the compiler
/// has begun resolving call names, the gate's `reify` tag now means what it
/// says, and the "green is weaker than it reads" paragraph in the module header
/// must be DELETED along with this test. It is here so that deletion is
/// prompted by a red test rather than depending on someone remembering.
#[test]
fn an_unresolved_call_name_compiles_clean_so_a_green_reify_tag_is_weaker() {
    let md = "```reify\n\
              pub trait Rigid : Physical {\n\
              \x20   let moment_of_inertia = no_such_helper_anywhere(geometry, 1.0)\n\
              }\n\
              ```\n";

    assert!(
        check_markdown("chunks/traits.md", md, reify_fence_violations).is_empty(),
        "the compiler still tolerates an unresolved call NAME, so this fixture \
         is expected to compile clean — if it now reports a violation, the \
         tolerance is gone: delete this test AND the \"green is weaker than it \
         reads\" paragraph in the module header, which would then be a false \
         disclaimer."
    );
}

/// The IDENTICAL phantom body under an exempt tag is never compiled BY THIS
/// CHECK.
///
/// This pins that the phantom is caught by the TAG contract and not
/// incidentally — and it is the property the whole retag sweep rests on. If
/// exempt tags were compiled anyway, retagging a fence would change nothing and
/// the sweep would be theatre; if bare `reify` were matched by prefix, every
/// `reify-fragment` in the corpus would be trial-compiled instead.
///
/// Scope note: exemption here is exemption from CHECK 1 only. `reify-invalid`
/// is separately verified by `reify_invalid_fence_violations` (check 3), which
/// compiles the body and demands the OPPOSITE verdict — so retagging a broken
/// `reify` fence to `reify-invalid` is not a free downgrade, it just swaps
/// which check owns it. The other two exempt tags make claims no gate can
/// falsify, and are exempt outright.
#[test]
fn the_same_phantom_body_under_an_exempt_tag_is_never_compiled() {
    let phantom = "structure def PhantomRotate {\n\
                   \x20   let blank = box(20mm, 20mm, 20mm)\n\
                   \x20   let turned = rotate(blank, vec3(0.0, 0.0, 1.0), 45deg)\n\
                   }";

    // Control: bare `reify` DOES catch it (same body, one tag apart).
    assert_eq!(
        check_markdown(
            "chunks/x.md",
            &format!("```reify\n{phantom}\n```\n"),
            reify_fence_violations
        )
        .len(),
        1,
        "control: the bare `reify` tag must still catch the phantom"
    );

    for tag in ["reify-schematic", "reify-fragment", "reify-invalid", "text"] {
        let md = format!("```{tag}\n{phantom}\n```\n");
        assert!(
            check_markdown("chunks/x.md", &md, reify_fence_violations).is_empty(),
            "tag `{tag}` is exempt and its body must never reach the compiler — \
             one tag apart from a body that IS reported"
        );
    }
}

/// A fence with a genuine PARSE error is a NAMED violation, not an
/// unattributed panic.
///
/// `compile_source_with_stdlib` (helpers.rs:236) panics on parse errors, which
/// would abort the whole gate with a backtrace naming no file and no fence —
/// defeating the "names file + fence ordinal + diagnostics" contract at exactly
/// the moment it matters most. The `_allow_parse_errors` variant folds parse
/// errors into `.diagnostics` at Error severity instead, so a malformed fence
/// reports like any other violation.
///
/// The fixture is empty-brace construction, which `enums.md` itself documents
/// as a GRAMMAR-level restriction: "`Point {}` reports `Parse error: syntax
/// error: {}` — write the bare variant as `Point`". A merely-unbalanced brace
/// will not do — the parser recovers from those and emits no parse error at
/// all, so it would exercise the compile path rather than the parse path this
/// test exists to cover.
#[test]
fn a_reify_fence_with_a_parse_error_is_a_named_violation_not_a_panic() {
    let md = "```reify\n\
              structure def Broken {\n\
              \x20   let p = Point {}\n\
              }\n\
              ```\n";

    let violations = check_markdown("chunks/x.md", md, reify_fence_violations);

    assert_eq!(
        violations.len(),
        1,
        "a malformed fence must be reported, not panicked on; got {violations:#?}"
    );
    assert!(
        violations[0].contains("chunks/x.md") && violations[0].contains("fence #1"),
        "even a parse failure must be attributed to file + fence ordinal, got: {}",
        violations[0]
    );
}

/// The violation echoes the fence body, so a failure is fixable without
/// re-opening the chunk — the same courtesy
/// `geometry_chunk_smoke::assert_module_compiles` already extends.
#[test]
fn the_violation_echoes_the_offending_fence_body() {
    let md = "```reify\n\
              structure def PhantomRotate {\n\
              \x20   let turned = rotate(box(1mm, 1mm, 1mm), vec3(0.0, 0.0, 1.0), 45deg)\n\
              }\n\
              ```\n";

    let violations = check_markdown("chunks/x.md", md, reify_fence_violations);
    assert!(
        violations[0].contains("structure def PhantomRotate"),
        "the fence body must be echoed in the violation, got: {}",
        violations[0]
    );
}

// ---------------------------------------------------------------------------
// Check 3 — a ```reify-invalid fence must ACTUALLY be invalid
//
// The mirror image of check 1, on the same synthetic fixtures: where check 1
// reports a `reify` fence that FAILS to compile, this reports a
// `reify-invalid` fence that SUCCEEDS. Hermetic for the same reason — the
// contract is pinned independently of what the corpus happens to hold today.
// ---------------------------------------------------------------------------

/// A `reify-invalid` fence whose body compiles clean is reported.
///
/// The tag claims the error is the lesson. When there is no error, the claim is
/// false and the reader is being shown a "do not write this" sample the
/// compiler is perfectly happy with.
#[test]
fn a_reify_invalid_fence_that_compiles_clean_is_reported() {
    let md = "prose\n\
              ```reify-invalid\n\
              structure def PerfectlyFine {\n\
              \x20   let blank = box(20mm, 20mm, 20mm)\n\
              }\n\
              ```\n";

    let violations = check_markdown("chunks/x.md", md, reify_invalid_fence_violations);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    let message = &violations[0];
    assert!(
        message.contains("chunks/x.md") && message.contains("fence #1") && message.contains(":2"),
        "the violation must name file, fence ORDINAL and OPENING line like every \
         other check, got: {message}"
    );
    assert!(
        message.contains("compiles CLEAN"),
        "the violation must say WHAT is wrong — that it compiled — got: {message}"
    );
    assert!(
        message.contains("structure def PerfectlyFine"),
        "the body must be echoed so the fence is fixable without re-opening the \
         chunk, got: {message}"
    );
}

/// A `reify-invalid` fence whose body does not PARSE is reported.
///
/// The vacuity this arm closes, pinned on the shape that produced it: a pair of
/// bare top-level `let`s referencing unbound names. That body DOES yield Error
/// diagnostics, so the un-hardened check was satisfied — but they are the
/// parser's, identical to what arbitrary prose yields, and the dimensional
/// lesson the sample was written to teach is never reached. A tag that a
/// syntax error can satisfy teaches nothing and protects nothing.
#[test]
fn a_reify_invalid_fence_that_only_fails_to_parse_is_reported() {
    let md = "```reify-invalid\n\
              let theta : Angle = s / r\n\
              ```\n";

    let violations = check_markdown("chunks/x.md", md, reify_invalid_fence_violations);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    let message = &violations[0];
    assert!(
        message.contains("does not PARSE"),
        "the violation must name the LAYER that rejected the body — a parse \
         rejection and a type error are opposite verdicts on the tag's claim, \
         and a message that blurs them sends the author to the wrong fix, got: \
         {message}"
    );
    assert!(
        message.contains("complete module"),
        "the violation must say what would fix it, got: {message}"
    );
}

/// Arbitrary prose under `reify-invalid` is reported, for the same reason.
///
/// The reductio the arm above exists to make impossible: before it, a body with
/// no reify in it at all satisfied the tag as fully as any real teaching
/// sample, so `REIFY_INVALID_FENCE_FLOOR` could be met by text.
#[test]
fn prose_under_reify_invalid_does_not_satisfy_the_tag() {
    let md = "```reify-invalid\n\
              zzz not reify at all !!!\n\
              ```\n";

    assert_eq!(
        check_markdown("chunks/x.md", md, reify_invalid_fence_violations).len(),
        1,
        "prose must not be able to satisfy a tag that claims the COMPILER's \
         verdict is the lesson"
    );
}

/// A `reify-invalid` fence that genuinely errors is NOT reported.
///
/// The control. Reusing the phantom 3-arg `rotate` makes the pairing exact:
/// the SAME body is a violation under `reify` (check 1) and clean under
/// `reify-invalid` (check 3), which is precisely what the two tags mean.
#[test]
fn a_reify_invalid_fence_that_genuinely_errors_is_not_reported() {
    let md = "```reify-invalid\n\
              structure def PhantomRotate {\n\
              \x20   let blank = box(20mm, 20mm, 20mm)\n\
              \x20   let turned = rotate(blank, vec3(0.0, 0.0, 1.0), 45deg)\n\
              }\n\
              ```\n";

    assert!(
        check_markdown("chunks/x.md", md, reify_invalid_fence_violations).is_empty(),
        "a deliberate-error sample that DOES error is exactly what the tag \
         claims — it must not be reported"
    );
}

/// Check 3 compiles ONLY `reify-invalid`, matched exactly.
///
/// The symmetric guard to `hyphenated_tags_are_exact_and_never_collapse_to_bare_reify`:
/// a clean body under any other tag — including bare `reify`, which check 1
/// already owns — must not be dragged into this check and reported for the
/// crime of compiling.
#[test]
fn check_three_never_reports_a_fence_under_any_other_tag() {
    let clean = "structure def PerfectlyFine {\n\
                 \x20   let blank = box(20mm, 20mm, 20mm)\n\
                 }";

    // Control: under `reify-invalid` the SAME clean body IS reported.
    assert_eq!(
        check_markdown(
            "chunks/x.md",
            &format!("```reify-invalid\n{clean}\n```\n"),
            reify_invalid_fence_violations
        )
        .len(),
        1,
        "control: a clean body under `reify-invalid` must be reported"
    );

    for tag in ["reify", "reify-fragment", "reify-schematic", "text"] {
        let md = format!("```{tag}\n{clean}\n```\n");
        assert!(
            check_markdown("chunks/x.md", &md, reify_invalid_fence_violations).is_empty(),
            "tag `{tag}` is not `reify-invalid` and must never be reported by \
             check 3 — one tag apart from a body that IS"
        );
    }
}

// ---------------------------------------------------------------------------
// Check 4 — every chunk is reachable through the MCP tool
//
// Hermetic: the fixtures below are a synthetic stem list plus synthetic
// `language_chunks.rs` source text. Nothing on disk is read.
// ---------------------------------------------------------------------------

/// A synthetic `language_chunks.rs` wired for exactly `stems`, in the same
/// shape as the real file: `include_str!` consts, a `TOPICS` slice literal, and
/// a `get_chunk` match — including the match, because a scan that is not scoped
/// to the TOPICS literal would be satisfied by the match arm instead.
fn synthetic_language_chunks_rs(include_str_stems: &[&str], topics: &[&str]) -> String {
    let consts = include_str_stems
        .iter()
        .map(|s| format!("const {}: &str = include_str!(\"chunks/{s}.md\");", s.to_uppercase()))
        .collect::<Vec<_>>()
        .join("\n");
    let topic_entries = topics
        .iter()
        .map(|s| format!("    \"{s}\","))
        .collect::<Vec<_>>()
        .join("\n");
    let arms = include_str_stems
        .iter()
        .map(|s| format!("        \"{s}\" => Some({}),", s.to_uppercase()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "// Language reference chunks\n\n{consts}\n\n\
         pub const TOPICS: &[&str] = &[\n{topic_entries}\n];\n\n\
         pub fn get_chunk(topic: &str) -> Option<&'static str> {{\n\
         \x20   match topic {{\n{arms}\n        _ => None,\n    }}\n}}\n"
    )
}

/// A chunk file that is never `include_str!`-ed is whole-file omission drift:
/// it ships in the repo, is served to nobody, and rots unread.
#[test]
fn a_stem_that_is_never_include_str_ed_is_reported() {
    let src = synthetic_language_chunks_rs(&["syntax", "units"], &["syntax", "units"]);
    let stems = vec!["syntax".to_string(), "units".to_string(), "ghost".to_string()];

    let violations = reachability_violations(&stems, &src);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("ghost"),
        "the violation must name the stem, got: {}",
        violations[0]
    );
    assert!(
        violations[0].contains("include_str!"),
        "the violation must say WHICH of the two references is missing, got: {}",
        violations[0]
    );
}

/// The subtler half: a stem that IS `include_str!`-ed and IS reachable through
/// `get_chunk`, but is absent from `TOPICS`.
///
/// Such a chunk compiles into the binary and even answers a direct lookup, yet
/// it is invisible through the `reify_language_reference` MCP tool, because
/// `TOPICS` is what the tool enumerates. Catching it REQUIRES scoping the scan
/// to the `TOPICS` slice literal — the fixture deliberately carries a
/// `"ghost" => Some(GHOST)` match arm, which a whole-file quoted-stem scan
/// would happily accept.
#[test]
fn a_stem_include_str_ed_but_absent_from_the_topics_literal_is_reported() {
    let src = synthetic_language_chunks_rs(&["syntax", "ghost"], &["syntax"]);
    let stems = vec!["syntax".to_string(), "ghost".to_string()];

    let violations = reachability_violations(&stems, &src);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("ghost") && violations[0].contains("TOPICS"),
        "the violation must name the stem and say TOPICS is the missing half, \
         got: {}",
        violations[0]
    );
}

/// A stem wired in BOTH places is clean.
#[test]
fn a_stem_wired_in_both_places_is_not_reported() {
    let stems: Vec<String> = ["syntax", "units", "traits"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let src = synthetic_language_chunks_rs(&["syntax", "units", "traits"], &["syntax", "units", "traits"]);

    assert!(
        reachability_violations(&stems, &src).is_empty(),
        "a fully wired corpus must be clean"
    );
}

/// Matching is ANCHORED: a stem is never satisfied by a coincidental substring
/// of a longer stem.
///
/// `types` inside `prototypes`, and `purposes` sharing letters with it, is the
/// adversarial pair. An unanchored `src.contains("types")` would report the
/// corpus clean while `types.md` was served to nobody — the exact silent
/// failure this check exists to prevent.
#[test]
fn a_stem_is_never_satisfied_by_a_longer_stem_that_contains_it() {
    let src = synthetic_language_chunks_rs(&["prototypes", "purposes"], &["prototypes", "purposes"]);
    let stems = vec!["types".to_string()];

    let violations = reachability_violations(&stems, &src);

    assert_eq!(
        violations.len(),
        1,
        "`types` must NOT be satisfied by `prototypes`/`purposes`; got {violations:#?}"
    );
    assert!(
        violations[0].contains("types"),
        "got: {}",
        violations[0]
    );
}

/// If the `TOPICS` slice literal cannot be located at all, that is itself a
/// violation — never a silent pass.
///
/// The check's whole TOPICS half is a text scan anchored on that literal. If a
/// refactor moved or renamed it, an unanchored implementation would find zero
/// entries, conclude nothing is wired, or (worse) fall back to a whole-file
/// scan and conclude everything is. Failing loudly is the only safe answer.
#[test]
fn a_missing_topics_literal_is_itself_a_violation() {
    let src = "const SYNTAX: &str = include_str!(\"chunks/syntax.md\");\n";
    let stems = vec!["syntax".to_string()];

    let violations = reachability_violations(&stems, src);

    assert!(
        !violations.is_empty(),
        "a language_chunks.rs with no TOPICS literal must fail loudly, not pass \
         vacuously"
    );
    assert!(
        violations.iter().any(|v| v.contains("TOPICS")),
        "the violation must say the TOPICS literal is what could not be found, \
         got {violations:#?}"
    );
}

// ---------------------------------------------------------------------------
// The real-corpus gate
//
// Each test accumulates EVERY failure and panics once at the end (the
// `examples_smoke.rs` shape), so a single run surfaces the whole backlog
// instead of stopping at the first offender — which for a corpus this size is
// the difference between one fix cycle and sixty-six.
// ---------------------------------------------------------------------------

/// One chunk file, read and parsed ONCE.
///
/// The parse OUTCOME is carried rather than unwrapped. Panicking on the first
/// malformed file would stop the run at the alphabetically-first offender and
/// cost one fix cycle per malformed file — the very property this module's
/// header claims accumulate-then-report-all bought.
struct ChunkDoc {
    /// The file stem (`traits`, `units`, …). Carried so a floor lookup keys on
    /// the same identity `REIFY_FENCE_FLOORS` does, rather than reconstructing
    /// a label and comparing strings.
    stem: String,
    label: String,
    fences: Result<Vec<Fence>, String>,
}

impl ChunkDoc {
    /// The fences that actually parsed; an empty slice for a file that did
    /// not. Counting is always over what was parsed — a parse failure is
    /// reported as its own violation, never disguised as missing fences.
    fn parsed(&self) -> &[Fence] {
        match &self.fences {
            Ok(fences) => fences,
            Err(_) => &[],
        }
    }

    /// Fences tagged EXACTLY ```` ```reify ```` in this file.
    fn bare_reify_fences(&self) -> usize {
        self.count_tagged("reify")
    }

    /// Fences tagged EXACTLY ```` ```reify-invalid ```` in this file.
    fn reify_invalid_fences(&self) -> usize {
        self.count_tagged("reify-invalid")
    }

    fn count_tagged(&self, tag: &str) -> usize {
        self.parsed()
            .iter()
            .filter(|fence| fence.tag.as_deref() == Some(tag))
            .count()
    }
}

/// Every chunk on disk, in stem order, parsed.
fn corpus() -> Vec<ChunkDoc> {
    discover_chunk_stems()
        .into_iter()
        .map(|stem| ChunkDoc {
            label: chunk_label(&stem),
            fences: parse_fences(&read_chunk_file(&stem)),
            stem,
        })
        .collect()
}

/// The PER-FILE bare-```` ```reify ```` floor: every chunk carrying at least
/// one such fence, each entry EQUAL to that file's live count.
///
/// EXACT, not a lower bound, and that is a standing obligation rather than a
/// snapshot of what was measured once. An entry set under live is no safety
/// margin: the fences above it are guarded by nothing, because retagging them
/// leaves the floor satisfied while the corpus-wide backstop below — the SUM of
/// these entries — goes slack by the same amount. Any diff that changes a fence
/// TAG therefore re-measures the file it touched and records the new count
/// here; `reify_fence_floors_are_exact_not_slack` enforces it and names the file
/// that drifted.
///
/// A single corpus-wide floor is not enough. The sweep's one silent-damage mode
/// is hollowing a passing test rather than failing one, and a slack aggregate
/// floor permits exactly that: with only a `>= 2` corpus-wide floor, retagging
/// nine of the eleven fences to `reify-fragment` would drop over 80% of the
/// gate's compile coverage and leave `every_reify_tagged_fence_compiles_clean`
/// green.
/// Pinning per file also ATTRIBUTES a loss to the file that took it, instead of
/// reporting a corpus total that says nothing about where to look.
///
/// Lowering an entry is a legitimate move — a fence can genuinely stop being a
/// standalone example — but it must be a deliberate, diff-visible edit HERE,
/// reviewed alongside the retag that motivated it.
///
/// # The table is TOTAL, and that is enforced
///
/// A per-file table that covers only some files is not a ratchet, because the
/// corpus-wide backstop below sums exactly these floors: a file the table does
/// not know about can grow three `reify` fences (total 11 → 14) and a later
/// task can retag all three away (14 → 11) with every assertion still passing,
/// and three documented examples silently stop being compiled. So
/// `assert_corpus_is_not_vacuous` requires that EVERY file carrying at least
/// one bare ```` ```reify ```` fence appears here. Adding such a fence to a new
/// file therefore forces a floor entry in the same diff, which is what makes
/// the aggregate check unable to mask a loss.
const REIFY_FENCE_FLOORS: &[(&str, usize)] = &[
    ("enums", 2),
    ("geometry", 4),
    ("purposes", 1),
    ("traits", 3),
    ("units", 1),
];

/// The corpus-wide floor on ```` ```reify-invalid ```` fences.
///
/// `every_reify_invalid_fence_actually_errors` compiles exactly these bodies
/// and nothing else, so at zero it protects nothing while still reporting
/// green. One is the count measured after this task's sweep (`units.md`'s
/// dimension-crossing sample). Not a per-file table like the `reify` one: the
/// tag is rare enough that a corpus total still attributes a loss unambiguously,
/// and a per-file entry would freeze WHICH chunk gets to teach by counterexample.
/// The EXACT number of `.md` files in the chunks dir, and the EXACT number of
/// fences across them.
///
/// Both are live counts, held to the same standard as `REIFY_FENCE_FLOORS` and
/// for the same reason: slack is not a safety margin. At `>= 16` against 17
/// files a whole chunk could be deleted with nothing going red and no constant
/// to lower; at `>= 60` against 76 fences, sixteen could vanish. That is the
/// hollowing the per-file table exists to close, reappearing one level up.
///
/// They are compared with `>=` HERE because this function's job is to fail FAST
/// and specifically — a vacuous scan must not be reported as four unrelated
/// check failures. The EXACTNESS obligation is a separate, separately-named
/// test, `corpus_counts_are_exact_not_slack`, so a diff that legitimately adds
/// a chunk or a fence gets a message telling it to re-measure rather than a
/// vacuity warning describing a bug that did not happen.
const CHUNK_FILE_COUNT: usize = 17;
const TOTAL_FENCE_COUNT: usize = 76;

const REIFY_INVALID_FENCE_FLOOR: usize = 1;

/// ANTI-VACUITY. Asserted BEFORE every corpus check.
///
/// A gate whose entire purpose is catching omission drift can itself drift into
/// silence: a parser regression that discovers nothing would leave every loop
/// below iterating zero times and every check GREEN, protecting nothing. This
/// is the same defence `reify_tagged_fences_in_geometry_chunk_compile` already
/// carries for its own scrape, applied
/// to all three axes the checks depend on — files discovered, fences parsed,
/// and bare ```` ```reify ```` fences actually reached.
fn assert_corpus_is_not_vacuous(corpus: &[ChunkDoc]) {
    // A file that failed to PARSE contributes zero fences, which would drag the
    // counts below toward a misleading "the parser has regressed" verdict. Name
    // those files in every floor message so a reader is never sent hunting the
    // wrong bug; each is separately reported as an ordinary violation by the
    // check itself.
    let unparsed: Vec<&str> = corpus
        .iter()
        .filter(|doc| doc.fences.is_err())
        .map(|doc| doc.label.as_str())
        .collect();
    let context = if unparsed.is_empty() {
        String::new()
    } else {
        format!(
            " NOTE: {} file(s) did not PARSE and so contribute no fences ({}); \
             fix those first — the check itself reports each one as its own \
             violation.",
            unparsed.len(),
            unparsed.join(", ")
        )
    };

    assert!(
        corpus.len() >= CHUNK_FILE_COUNT,
        "the chunk-dir scan found only {} `.md` file(s) in {CHUNKS_DIR} — \
         expected {CHUNK_FILE_COUNT}. Either the scan is vacuous (dir moved, or \
         the glob broke) and the checks below protect NOTHING, or a chunk was \
         deleted and CHUNK_FILE_COUNT must come down in the same diff.{context}",
        corpus.len()
    );

    let total_fences: usize = corpus.iter().map(|doc| doc.parsed().len()).sum();
    assert!(
        total_fences >= TOTAL_FENCE_COUNT,
        "the fence scan found only {total_fences} fence(s) across {} chunk \
         file(s) — expected {TOTAL_FENCE_COUNT}. Either the parser has regressed \
         and every check below is passing trivially, or fences were legitimately \
         removed and TOTAL_FENCE_COUNT must come down in the same diff.{context}",
        corpus.len()
    );

    // PER-FILE bare-```reify floors, so a loss is attributed to the file that
    // took it rather than absorbed by a corpus total.
    for (stem, floor) in REIFY_FENCE_FLOORS {
        let label = chunk_label(stem);
        let found = corpus
            .iter()
            .find(|doc| doc.label == label)
            .map(ChunkDoc::bare_reify_fences)
            .unwrap_or(0);
        assert!(
            found >= *floor,
            "{label} carries {found} fence(s) tagged EXACTLY `reify`, expected at \
             least {floor}. Those fences are the ONLY bodies this gate actually \
             compiles, so retagging one to `reify-fragment` does not fail \
             `every_reify_tagged_fence_compiles_clean` — it HOLLOWS it, and a \
             documented example silently stops being checked. If the fence \
             genuinely stopped being a standalone example, fix the body or lower \
             this floor deliberately in REIFY_FENCE_FLOORS so the loss is \
             reviewable in the diff.{context}"
        );
    }

    // TABLE COMPLETENESS. Without this the per-file table is not a ratchet at
    // all for any file outside it, and the aggregate below — which sums exactly
    // these floors — can be satisfied by fences that arrived after the table was
    // written and were then retagged away. Requiring an entry for every file
    // that HAS `reify` fences closes the loop: a new file's fences cannot enter
    // the corpus without acquiring a floor in the same diff.
    for doc in corpus.iter().filter(|doc| doc.bare_reify_fences() > 0) {
        let found = doc.bare_reify_fences();
        assert!(
            REIFY_FENCE_FLOORS
                .iter()
                .any(|(stem, _)| *stem == doc.stem),
            "{} carries {found} fence(s) tagged EXACTLY `reify` but has NO entry \
             in REIFY_FENCE_FLOORS. Add `(\"{}\", {found})` there. Until it is \
             listed, those fences are protected by nothing: the corpus-wide \
             backstop is the SUM of the table's floors, so a later task can \
             retag every one of them back to `reify-fragment` and every \
             assertion here still passes while {found} documented example(s) \
             silently stop being compiled.{context}",
            doc.label, doc.stem
        );
    }

    let expected_reify: usize = REIFY_FENCE_FLOORS.iter().map(|(_, floor)| floor).sum();
    let reify_fences: usize = corpus.iter().map(ChunkDoc::bare_reify_fences).sum();
    assert!(
        reify_fences >= expected_reify,
        "the scan found only {reify_fences} bare ```reify fence(s) corpus-wide — \
         expected at least {expected_reify}, the sum of REIFY_FENCE_FLOORS. \
         `every_reify_tagged_fence_compiles_clean` compiles exactly these bodies \
         and nothing else, so every one lost is compile coverage lost with no \
         test going red.{context}"
    );

    // CHECK 3's anti-vacuity floor. `reify-invalid` is the one exempt tag that
    // makes a falsifiable claim, and check 3 verifies it — but only over fences
    // that exist. At zero the check is green and empty.
    let reify_invalid_fences: usize = corpus.iter().map(ChunkDoc::reify_invalid_fences).sum();
    assert!(
        reify_invalid_fences >= REIFY_INVALID_FENCE_FLOOR,
        "the scan found only {reify_invalid_fences} ```reify-invalid fence(s) \
         corpus-wide — expected at least {REIFY_INVALID_FENCE_FLOOR}. \
         `every_reify_invalid_fence_actually_errors` compiles exactly these \
         bodies, so at zero it reports green while verifying nothing. If a \
         deliberate-error sample was legitimately removed, lower \
         REIFY_INVALID_FENCE_FLOOR in the same diff so the loss is \
         reviewable.{context}"
    );
}

/// Render an accumulated violation list as one panic message.
fn report(check: &str, violations: &[String]) {
    assert!(
        violations.is_empty(),
        "{check}: {} violation(s)\n\n{}\n",
        violations.len(),
        violations.join("\n\n")
    );
}

/// CHECK 2 — no fence anywhere in the corpus is untagged.
#[test]
fn no_chunk_fence_is_untagged() {
    let corpus = corpus();
    assert_corpus_is_not_vacuous(&corpus);

    let violations: Vec<String> = corpus
        .iter()
        .flat_map(|doc| check_parse_outcome(&doc.label, &doc.fences, untagged_fence_violations))
        .collect();

    report(
        "untagged fences in the MCP language-reference chunks. Every fence must \
         declare what it IS, so a reader can tell a copy-pasteable example from \
         a schematic without trying it",
        &violations,
    );
}

/// CHECK 1 — every bare ```` ```reify ```` fence compiles standalone.
#[test]
fn every_reify_tagged_fence_compiles_clean() {
    let corpus = corpus();
    assert_corpus_is_not_vacuous(&corpus);

    let violations: Vec<String> = corpus
        .iter()
        .flat_map(|doc| check_parse_outcome(&doc.label, &doc.fences, reify_fence_violations))
        .collect();

    report(
        "```reify fences that do not compile. A fence carrying the bare `reify` \
         tag CLAIMS to be a complete, copy-pasteable module; if the compiler \
         rejects it, the doc is lying and the reader pays a probe cycle to find \
         out",
        &violations,
    );
}

/// CHECK 3 — every ```` ```reify-invalid ```` fence really does fail to compile.
#[test]
fn every_reify_invalid_fence_actually_errors() {
    let corpus = corpus();
    assert_corpus_is_not_vacuous(&corpus);

    let violations: Vec<String> = corpus
        .iter()
        .flat_map(|doc| {
            check_parse_outcome(&doc.label, &doc.fences, reify_invalid_fence_violations)
        })
        .collect();

    report(
        "```reify-invalid fences that compile clean. That tag says the ERROR is \
         the lesson; a body the compiler accepts teaches the opposite of what \
         the fence claims, and leaves the tag available as a silent downgrade \
         path for a `reify` fence that stopped compiling",
        &violations,
    );
}

/// CHECK 4 — every chunk on disk is reachable through the MCP tool.
#[test]
fn every_chunk_is_reachable_through_the_mcp_tool() {
    let stems = discover_chunk_stems();
    assert!(
        stems.len() >= 16,
        "the chunk-dir scan found only {} `.md` file(s) — the reachability \
         check is vacuous",
        stems.len()
    );

    let src = std::fs::read_to_string(LANGUAGE_CHUNKS_RS).unwrap_or_else(|e| {
        panic!("{LANGUAGE_CHUNKS_RS} must be readable ({e}) — update LANGUAGE_CHUNKS_RS if it moved")
    });

    report(
        "chunk files that are unreachable through `reify_language_reference`. A \
         chunk wired in neither place ships to nobody; one missing only from \
         TOPICS compiles in and answers `get_chunk` yet cannot be enumerated, \
         which is the drift a reader never sees",
        &reachability_violations(&stems, &src),
    );
}

/// Every floor in `REIFY_FENCE_FLOORS` must EQUAL its file's live count, not
/// merely sit at or under it.
///
/// `assert_corpus_is_not_vacuous` only ever asserts `live >= floor` and that an
/// entry EXISTS; neither looks at its VALUE, so nothing there stops an entry
/// going slack. Why slack is not a safety margin is argued on
/// `REIFY_FENCE_FLOORS` and restated in this test's own failure message.
///
/// The EXACT-count rule is imported, not invented:
/// `reify_tagged_fences_in_geometry_chunk_compile` already sets its own floor
/// "to the EXACT live count per the re-measurement protocol ... a floor under
/// live is the measured incident that protocol exists to prevent, not a safety
/// margin".
#[test]
fn reify_fence_floors_are_exact_not_slack() {
    let corpus = corpus();
    assert_corpus_is_not_vacuous(&corpus);

    for (stem, floor) in REIFY_FENCE_FLOORS {
        let doc = corpus.iter().find(|doc| doc.stem == *stem).unwrap_or_else(|| {
            panic!(
                "REIFY_FENCE_FLOORS records a floor of {floor} for `{stem}`, but \
                 {CHUNKS_DIR} holds no `{stem}.md`. The chunk was renamed or deleted \
                 without the table following, so that floor now guards nothing at all."
            )
        });
        let live = doc.bare_reify_fences();
        assert_eq!(
            live,
            *floor,
            "{} carries {live} fence(s) tagged EXACTLY `reify` while REIFY_FENCE_FLOORS \
             records {floor}. Entries here are EXACT live counts, never lower bounds: the \
             {} fence(s) above the floor are guarded by nothing — retagging them to \
             `reify-fragment` leaves this floor satisfied, and the corpus-wide backstop \
             (the SUM of this table) goes slack by the same amount, so that many \
             documented examples stop being compiled with no test going red. Re-measure \
             and record the live count in the SAME diff that changes a fence tag. \
             Lowering an entry deliberately is legitimate; drifting under one is the \
             incident this test exists to report.",
            doc.label,
            live.saturating_sub(*floor)
        );
    }
}

/// `CHUNK_FILE_COUNT` and `TOTAL_FENCE_COUNT` must EQUAL the live corpus.
///
/// The corpus-level twin of `reify_fence_floors_are_exact_not_slack`, and it
/// exists because that test's own argument — slack is not a safety margin —
/// applies just as well one level up. `assert_corpus_is_not_vacuous` compares
/// with `>=` so a vacuous scan fails fast; without this test that `>=` would be
/// the only comparison, and the gap between floor and live would be exactly the
/// number of chunks or fences that could disappear unremarked.
///
/// Growing the corpus is expected and makes this go red on purpose: raise the
/// constant in the diff that adds the file or fence. What must not happen
/// silently is the other direction.
#[test]
fn corpus_counts_are_exact_not_slack() {
    let corpus = corpus();
    assert_corpus_is_not_vacuous(&corpus);

    assert_eq!(
        corpus.len(),
        CHUNK_FILE_COUNT,
        "{CHUNKS_DIR} holds {} `.md` file(s) while CHUNK_FILE_COUNT records \
         {CHUNK_FILE_COUNT}. Re-measure and record the live count in the SAME \
         diff that adds or removes a chunk — otherwise the difference is the \
         number of chunks that can later vanish with every test still green.",
        corpus.len()
    );

    let total_fences: usize = corpus.iter().map(|doc| doc.parsed().len()).sum();
    assert_eq!(
        total_fences, TOTAL_FENCE_COUNT,
        "the corpus holds {total_fences} fence(s) while TOTAL_FENCE_COUNT \
         records {TOTAL_FENCE_COUNT}. Re-measure and record the live count in \
         the SAME diff that adds or removes a fence; the difference is the \
         number of fences that can later vanish unremarked."
    );
}

// ---------------------------------------------------------------------------
// CROSS-HARNESS PIN
//
// A retag sweep's damaging move is never a failing test — it is a PASSING one
// that quietly stopped protecting anything. The sibling geometry suite defends
// itself against that with a floor at its EXACT live count, so a retag fails it
// loudly. This pin adds the three things that floor cannot: attribution inside
// the retag's own diff, a second literal that has to be lowered deliberately
// alongside the sibling's, and an agreement check between the two harnesses'
// idea of what a ```reify fence is.
// ---------------------------------------------------------------------------

/// The sibling suite's OWN scanner, with this pin's arguments bound once.
///
/// A call, not a copy. What this replaced claimed to reproduce
/// `reify_tagged_fences` verbatim so the two could be seen to drift apart, but
/// never did: the real one has been tag-parameterized since task 5759 and
/// carries an unterminated-fence assert the copy lacked, so the
/// drift-detection rationale did not hold. Calling it makes this pin exercise
/// the ACTUAL coupling and turns a rename or signature change over there into
/// a compile error here rather than silent rot.
fn sibling_reify_fence_count(markdown: &str) -> usize {
    reify_tagged_fences(markdown, "reify", &chunk_label("geometry")).len()
}

/// The stem whose bare-```` ```reify ```` fences the sibling suite compiles.
const SIBLING_GEOMETRY_STEM: &str = "geometry";

/// The sibling suite's own anti-vacuity floor on `geometry.md`'s bare
/// ```` ```reify ```` fences, so a retag sweep has to lower TWO deliberate
/// literals rather than walk under one.
///
/// READ OUT of `REIFY_FENCE_FLOORS` rather than restated. Both this pin and
/// that table describe the same quantity — how many bare ```` ```reify ````
/// fences `geometry.md` carries — and a second literal spelling it could drift
/// from the first while every test stayed green, leaving the pin to fail for a
/// reason its own message misdescribes. One literal, in the table that already
/// owns per-file counts and that `reify_fence_floors_are_exact_not_slack`
/// already holds to the EXACT live value.
///
/// The sibling's own inline `fences.len() >= 4` remains a genuinely
/// independent literal over in `reify_tagged_fences_in_geometry_chunk_compile`,
/// which is what makes this a mirror of something rather than a restatement of
/// itself. Nothing here can enforce equality with it — it is a local in another
/// module — so the pin's failure message names it explicitly as the second
/// place to look.
fn sibling_geometry_reify_fence_floor() -> usize {
    REIFY_FENCE_FLOORS
        .iter()
        .find(|(stem, _)| *stem == SIBLING_GEOMETRY_STEM)
        .map(|(_, floor)| *floor)
        .unwrap_or_else(|| {
            panic!(
                "REIFY_FENCE_FLOORS has no `{SIBLING_GEOMETRY_STEM}` entry, but a \
                 sibling suite compiles that file's bare ```reify fences and this \
                 pin mirrors its floor. Removing the entry does not retire the \
                 coupling — it hides it. Restore the entry at the file's exact \
                 live count, or retire the sibling's subject and this pin together."
            )
        })
}

/// Does `markdown` still carry enough bare ```` ```reify ```` fences to keep the
/// sibling suite's compile subjects?
///
/// A named predicate rather than an inline comparison, so the pin below and the
/// hermetic controls that falsify it share ONE floor. A control that re-spelled
/// the comparison could drift away from the assertion it claims to exercise,
/// which is the same class of defect this whole pin exists to catch.
fn meets_sibling_geometry_reify_floor(markdown: &str) -> bool {
    sibling_reify_fence_count(markdown) >= sibling_geometry_reify_fence_floor()
}

/// `geometry.md` must keep ALL FOUR of its bare ```` ```reify ```` fences,
/// because a sibling suite in this same compile unit selects them by that exact
/// string and compiles what it finds — the coupling the module header sets out.
///
/// A retag over there therefore fails LOUDLY already. This pin is NOT a
/// backstop against a silent loss; read it as adding three things the sibling's
/// floor cannot:
///
/// - ATTRIBUTION IN THE RETAG'S OWN DIFF. The sibling reports a count from a
///   file whose subject is geometry queries; this test names the retag as the
///   cause, in the module whose subject is fence tags.
/// - A SECOND DELIBERATE LITERAL. `geometry`'s `REIFY_FENCE_FLOORS` entry has
///   to be lowered alongside the sibling's own inline floor, so retiring a
///   compile subject is a decision taken twice rather than a number walked down
///   once. See `sibling_geometry_reify_fence_floor` for why this side reads
///   that entry instead of spelling a third copy of the same count.
/// - THE AGREEMENT CHECK, which nothing else performs: the sibling's real
///   scanner and this module's parser must find the SAME fences. Either side
///   alone can be green while the two harnesses have already drifted apart on
///   what ```` ```reify ```` means.
#[test]
fn geometry_chunk_retains_bare_reify_fences_for_the_sibling_smoke_suite() {
    let content = read_chunk_file("geometry");
    let label = chunk_label("geometry");

    assert!(
        meets_sibling_geometry_reify_floor(&content),
        "{label} carries only {} fence(s) tagged EXACTLY `reify`, expected {} — \
         the floor \
         `reify_tagged_fences_in_geometry_chunk_compile` asserts for itself over this same \
         file. That suite compiles each of these fences \
         VERBATIM, so the retag that produced this failure is failing it too: expect two \
         red tests, and do not read this one as the whole consequence. If a fence genuinely \
         stopped compiling standalone, fix the fence — or retire the sibling's subject \
         deliberately and lower BOTH floors in the same diff. Do NOT quietly retag it to \
         `reify-fragment`.",
        sibling_reify_fence_count(&content),
        sibling_geometry_reify_fence_floor()
    );

    let parsed_bare_reify = parse_fences(&content)
        .unwrap_or_else(|e| panic!("{label}: {e}"))
        .into_iter()
        .filter(|f| f.tag.as_deref() == Some("reify"))
        .collect::<Vec<_>>();

    let scraped = sibling_reify_fence_count(&content);
    assert_eq!(
        scraped,
        parsed_bare_reify.len(),
        "the sibling's exact-string scrape finds {scraped} bare ```reify opening line(s) in \
         {label} but this module's parser finds {}. The two harnesses have DRIFTED: whatever \
         one of them now believes is a `reify` fence, the other does not. Reconcile them \
         before touching any tag.",
        parsed_bare_reify.len()
    );

    // NEGATIVE CONTROL, hermetic — proves the assertions above can actually go
    // RED. Retag geometry.md's own opening delimiters in memory (the real file
    // is never written) and confirm BOTH sides stop counting them, i.e. that
    // `reify-fragment` is not swept in by a prefix match on either side.
    let retagged = content.replace("\n```reify\n", "\n```reify-fragment\n");
    assert_ne!(
        retagged, content,
        "the negative control rewrote nothing — its `\\n```reify\\n` pattern no longer matches \
         {label}, so it is proving nothing and must be updated with the file"
    );
    assert_eq!(
        sibling_reify_fence_count(&retagged),
        0,
        "the sibling scrape still counted bare `reify` fences after every one was retagged to \
         `reify-fragment` — its exact match has become a prefix match, and `reify-fragment` / \
         `reify-schematic` bodies are now being compiled as if they were standalone modules"
    );
    assert_eq!(
        parse_fences(&retagged)
            .unwrap_or_else(|e| panic!("{label} (retagged): {e}"))
            .into_iter()
            .filter(|f| f.tag.as_deref() == Some("reify"))
            .count(),
        0,
        "this module's parser still reported fences tagged `reify` after every one was retagged \
         to `reify-fragment` — the EXACT-match tag contract has regressed to a prefix match"
    );

    // NEGATIVE CONTROL, PARTIAL — the case the control above cannot reach. A
    // sweep that empties the file is caught by any floor at all; the damaging
    // one retags SOME fences and leaves the rest, and a floor set under the live
    // count accepts exactly that. Same hermetic shape: the real file is never
    // written.
    let live = sibling_reify_fence_count(&content);
    let partly_retagged = content.replacen("\n```reify\n", "\n```reify-fragment\n", 2);
    assert_eq!(
        sibling_reify_fence_count(&partly_retagged) + 2,
        live,
        "the partial control did not retag exactly two opening delimiters in {label} — its \
         `\\n```reify\\n` pattern no longer matches the file the way it assumes, so it is not \
         exercising the case it names and must be updated with the file"
    );
    assert!(
        !meets_sibling_geometry_reify_floor(&partly_retagged),
        "retagging two of {label}'s {live} bare ```reify fences to `reify-fragment` still \
         satisfies the mirrored floor ({}). \
         A floor under the live count pins nothing above itself: those two fences could be \
         retagged in any future sweep and this pin — the one test whose whole purpose is \
         naming that retag as the cause — would stay green. Raise the floor to the sibling \
         suite's own live count.",
        sibling_geometry_reify_fence_floor()
    );
}

