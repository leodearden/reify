/**
 * Pure helpers for the event-channel Consumer-column contract. The invariant
 * they exist to enforce, and why a gap in it fails silently today, are
 * documented once in the consumer — `eventChannelConsumerCoverage.test.ts`.
 * Not restated here.
 *
 * Deliberately free of any vitest import, and the parsing half is free of
 * `node:fs`: everything below `readEventChannelInventory` is a pure function
 * over plain data, so the fragile half (markdown-table parsing) is directly
 * unit-testable with synthetic input — see `eventChannelConsumerContract.test.ts`.
 *
 * This file has no `.test.` segment, so vitest's default `*.{test,spec}.*`
 * include does not collect it as a suite (same convention as
 * `bridgeMockContract.ts` and `toolDefNames.ts`). It lives under `gui/src`, so
 * `gui/tsconfig.json`'s `include: ["src"]` still puts it in tsc's strict
 * program.
 */
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Read docs/gui-event-channels.md from the repo root.
 *
 * The path is resolved from THIS module's own location, not the caller's:
 * `import.meta.url` is per-module, so one computation is correct for every
 * importer regardless of which directory it sits in. Same idiom as
 * `toolDefNames.ts`'s `readDebugServerSource`, which documents why naive
 * URL-relative `..` math over-shoots. Reading a REPO-ROOT file (outside gui/)
 * from this directory is precedented by `reifyGrammarCorpus.test.ts`.
 *
 * Segment count:
 *
 *   <repo>/gui/src/__tests__/eventChannelConsumerContract.ts
 *                  ^^^^^^^^^   (dirname = __tests__/)
 *              ^^^               (..     = src/)
 *          ^^^                   (..     = gui/)
 *   ^^^^^^                       (..     = repo root)
 */
export function readEventChannelInventory(): string {
  const dir = path.dirname(fileURLToPath(import.meta.url));
  return readFileSync(path.resolve(dir, '..', '..', '..', 'docs', 'gui-event-channels.md'), 'utf-8');
}

/**
 * The published row contract from docs/gui-event-channels.md §preamble:
 * "Every event channel name in column 1 of §1 and §2 is wrapped in single
 * backticks so the regex `\| \`[a-z0-9-]+\` \|` matches every event-channel row
 * machine-grep-style."
 *
 * Carried here anchored at line start, since column 1 is where a channel name
 * lives; `scripts/check_event_inventory.sh`'s awk scans the same pattern
 * unanchored. Reusing the doc's own contract rather than inventing a second
 * notion of "what is a channel row" is what keeps this guard and that lint
 * looking at exactly one row set.
 */
const CHANNEL_ROW_RE = /^\|\s*`([a-z0-9-]+)`\s*\|/;

/** Same pattern, unanchored — the awk semantic, used for the independent recount. */
export const CHANNEL_ROW_SCAN_RE = /\|\s*`[a-z0-9-]+`\s*\|/;

/**
 * Zero-based index of the Consumer cell in the array `splitTableCells` returns.
 *
 * Index 0 is the empty span before a row's leading pipe, so the Nth data cell
 * sits at index N. Consumer is the 4th data cell in BOTH table shapes —
 * §1 is `Channel | Payload | Producer | Consumer | Notes` and §2 is
 * `Channel | Payload | Producer | Consumer | Upstream prereq | Owning slice |
 * Spec` — so one index serves both.
 *
 * That coincidence is CHECKED, not assumed: `parseEventChannelRows` reads a
 * table's rows only after seeing a header whose cell at this index is literally
 * `Consumer` (see `CONSUMER_HEADING`). Inserting a column ahead of Consumer
 * would otherwise silently repoint this guard at the Producer column. Today that
 * happens to fail loudly — every live Producer cell is either a `::`-bearing
 * path or a snake_case name, so it degrades to `needs-allowlist` — but that is
 * incidental to the current doc content: a future producer written
 * `` `emitStatus` `` is a real bridge-shaped lowercase identifier and would sail
 * through as if it were the consumer.
 */
const CONSUMER_CELL_INDEX = 4;

/** The literal heading text that must sit at `CONSUMER_CELL_INDEX`. */
const CONSUMER_HEADING = 'Consumer';

/**
 * A markdown table's `|---|---|` alignment row.
 *
 * Used as the STRUCTURAL marker of a table's shape: markdown puts it directly
 * below the header row, so the line above one is the header and everything else
 * starting with `|` is a data row. That is a name-class-independent way to find
 * both, which matters — see `tableDataRows`.
 */
const TABLE_SEPARATOR_RE = /^\|[\s:|-]+\|\s*$/;

/**
 * Split one markdown table row into cells on UNESCAPED pipes, trimming each
 * cell and resolving `\|` to the literal pipe the markdown renders.
 *
 * The leading and trailing empty spans either side of a row's outer pipes are
 * PRESERVED, so the Nth data cell is at index N — see `CONSUMER_CELL_INDEX`.
 *
 * The escape handling is here because of a real row, not defensively: the
 * `warm-pool-event` row (docs/gui-event-channels.md:52) carries `\|` inside its
 * Payload cell (`WarmPoolEvent {kind: 'evicted'\|'donated', …}`). A plain
 * `split('|')` — or `awk -F'|'` — shifts that row's later columns by one and
 * reads the PRODUCER cell as the Consumer, silently dropping `onWarmPoolEvent`
 * from coverage while the guard still reports green. The consumer suite pins
 * `warm-pool-event -> onWarmPoolEvent` by name so a future simplification of
 * this splitter cannot quietly reintroduce that.
 */
export function splitTableCells(row: string): string[] {
  const cells: string[] = [];
  let current = '';
  for (let i = 0; i < row.length; i += 1) {
    const ch = row[i];
    if (ch === '\\' && row[i + 1] === '|') {
      current += '|';
      i += 1;
      continue;
    }
    if (ch === '|') {
      cells.push(current.trim());
      current = '';
      continue;
    }
    current += ch;
  }
  cells.push(current.trim());
  return cells;
}

/** A parsed §1/§2 channel row: its section, its name, and its raw Consumer cell. */
export interface EventChannelRow {
  section: '§1' | '§2';
  /** Channel name with the backticks stripped, e.g. `mesh-update`. */
  channel: string;
  /** The Consumer cell verbatim (trimmed, escapes resolved). */
  consumerCell: string;
}

/**
 * Which §-section a heading line opens, or `null` when the line is not a
 * top-level §-heading this parser tracks.
 */
function sectionOf(line: string): EventChannelRow['section'] | null {
  if (line.startsWith('## §1 ')) return '§1';
  if (line.startsWith('## §2 ')) return '§2';
  return null;
}

/**
 * Parse every §1/§2 channel row out of the inventory markdown, in document
 * order.
 *
 * The section walk mirrors `extract_registered_channels`'s `sec1_only` awk in
 * scripts/check_event_inventory.sh: key on `^## §N` headings, and additionally
 * reset on any `^### ` heading. The `### ` reset is what excludes §2a
 * STRUCTURALLY as well as by the backtick contract — §2a's rows use bold
 * first-column formatting (`| **solver-cancel-request** |`) precisely so they
 * fall outside the grep contract, and the doc states that normatively, but
 * belt-and-braces here costs one line and removes the need to reason about
 * which of the two exclusions is load-bearing.
 *
 * Rows that match neither the section scope nor the contract regex — §3's
 * snake_case RPC names, §4/§5 content, header rows, `|---|` separators, prose —
 * yield nothing. An empty document yields `[]` rather than throwing, so a
 * mis-targeted read trips the consumer's non-vacuity checks with a clear
 * message rather than an opaque parser error.
 *
 * Rows are emitted only from a table whose header puts `Consumer` at
 * `CONSUMER_CELL_INDEX`; a table whose columns have been reordered or extended
 * on the left yields nothing rather than the wrong column. Fail-CLOSED rather
 * than throwing, again: the dropped rows surface through the consumer suite's
 * row-count equality and non-vacuity checks, and `eventChannelTableHeaders`
 * lets that suite name the offending header directly.
 */
export function parseEventChannelRows(markdown: string): EventChannelRow[] {
  const rows: EventChannelRow[] = [];
  let section: EventChannelRow['section'] | null = null;
  // Cells of the last `|` line that was neither a separator nor a channel row —
  // i.e. the header candidate — and whether the separator below it confirmed a
  // `Consumer` column at the expected index.
  let headerCandidate: string[] | null = null;
  let consumerColumnConfirmed = false;

  for (const line of markdown.split('\n')) {
    if (line.startsWith('### ') || line.startsWith('## ')) {
      section = line.startsWith('## ') ? sectionOf(line) : null;
      headerCandidate = null;
      consumerColumnConfirmed = false;
      continue;
    }
    if (section === null || !line.startsWith('|')) continue;

    if (TABLE_SEPARATOR_RE.test(line)) {
      consumerColumnConfirmed =
        headerCandidate !== null && headerCandidate[CONSUMER_CELL_INDEX] === CONSUMER_HEADING;
      headerCandidate = null;
      continue;
    }

    const match = CHANNEL_ROW_RE.exec(line);
    if (match === null) {
      headerCandidate = splitTableCells(line);
      continue;
    }
    if (!consumerColumnConfirmed) continue;

    const cells = splitTableCells(line);
    rows.push({
      section,
      channel: match[1],
      consumerCell: cells[CONSUMER_CELL_INDEX] ?? '',
    });
  }

  return rows;
}

/**
 * The cells of every markdown table header inside a §1/§2 section, in document
 * order — the header being, structurally, the `|` line directly above a
 * `|---|` separator.
 *
 * Exported so the consumer suite can assert the Consumer column's POSITION with
 * a message that names the header it found. `parseEventChannelRows` enforces the
 * same condition by refusing rows, which is the fail-safe half; this is the
 * legible half. A count mismatch alone would say "expected 0 to be 40" and leave
 * a maintainer to guess.
 */
export function eventChannelTableHeaders(markdown: string): { section: EventChannelRow['section']; cells: string[] }[] {
  const headers: { section: EventChannelRow['section']; cells: string[] }[] = [];
  const lines = markdown.split('\n');
  let section: EventChannelRow['section'] | null = null;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.startsWith('### ') || line.startsWith('## ')) {
      section = line.startsWith('## ') ? sectionOf(line) : null;
      continue;
    }
    if (section === null || !line.startsWith('|')) continue;
    if (TABLE_SEPARATOR_RE.test(line)) continue;
    if (i + 1 < lines.length && TABLE_SEPARATOR_RE.test(lines[i + 1])) {
      headers.push({ section, cells: splitTableCells(line) });
    }
  }

  return headers;
}

/**
 * Every markdown table DATA row in `markdown` — each line starting with `|` that
 * is neither a `|---|` separator nor the header directly above one.
 *
 * Deliberately knows nothing about channel names. It is the independent recount
 * the consumer suite compares `parseEventChannelRows` against, and an
 * independence that stopped at the section walk would be worth little: reusing
 * the same `[a-z0-9-]+` name class on both sides means a row the name class
 * fails to recognise — a channel added in snake_case, as §3 already uses, or
 * with any uppercase letter — is missed by BOTH counts, the equality still
 * holds, and the row is silently unguarded. Counting by table STRUCTURE instead
 * has no name class to share, so such a row shows up as a surplus data row and
 * fails.
 */
export function tableDataRows(markdown: string): string[] {
  const lines = markdown.split('\n');
  const dataRows: string[] = [];

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (!line.startsWith('|')) continue;
    if (TABLE_SEPARATOR_RE.test(line)) continue;
    if (i + 1 < lines.length && TABLE_SEPARATOR_RE.test(lines[i + 1])) continue;
    dataRows.push(line);
  }

  return dataRows;
}

/** Every backtick-delimited span in a cell, in source order. */
const BACKTICKED_SPAN_RE = /`([^`]*)`/g;

/** The prefix a Consumer cell uses when it spells out the module. */
const BRIDGE_PREFIX = 'bridge.ts::';

/** A JS identifier. Used only for the `bridge.ts::NAME` suffix. */
const IDENTIFIER_RE = /^[A-Za-z_$][\w$]*$/;

/**
 * A JS identifier whose FIRST character is lowercase.
 *
 * The lowercase requirement is the whole noise-rejection mechanism for bare
 * (unprefixed) spans, and it is deliberate rather than incidental: the Consumer
 * column legitimately carries PascalCase Solid component names —
 * `BucklingPanel`, `AutoResolvePanel`, `WarmPoolDebugPanel`,
 * `SolverProgressOverlay`, `FeaCasePickerDropdown` — which are real consumers
 * of the channel but are NOT bridge.ts exports. Excluding them by character
 * class costs nothing; excluding them by allowlist would cost five entries that
 * all say "not a bridge symbol", and an allowlist whose entries carry no
 * distinguishing reason has no signal left to give (the failure mode
 * bridgeMockCoverage.test.ts documents when it refuses to glob its target list).
 *
 * The dotted and slashed exclusions fall out of the same character class rather
 * than needing their own rule: `engineStore.setFeaDiagnostics`, `gui/src` and
 * `gui/src/debug/WarmPoolDebugPanel.tsx` all contain a character outside
 * `[\w$]`, so they fail the anchored match.
 *
 * Under-capture is not the dangerous direction here: a row whose Consumer cell
 * yields nothing is classified `needs-allowlist` and must then be either
 * allowlisted or fixed, so a missed identifier surfaces as a FAILING row rather
 * than a silently unguarded one.
 */
const LOWER_INITIAL_IDENTIFIER_RE = /^[a-z][\w$]*$/;

/**
 * Extract the bridge.ts consumer identifiers named by one Consumer cell, in
 * source order, deduplicated.
 *
 * Rule, per backticked span:
 *   - `bridge.ts::NAME` → take NAME, stripping one trailing `()`, and keep it
 *     when it is an identifier. The explicit module prefix is the author saying
 *     "this is a bridge symbol", so no case restriction is applied to it.
 *   - anything else → keep it only when it is a bare identifier starting with a
 *     lowercase letter (see `LOWER_INITIAL_IDENTIFIER_RE` for why).
 *
 * Unbackticked prose (`same`, `(new)`, `animator`, `debug-bridge`) is never
 * considered: only backticked spans are candidates.
 */
export function extractConsumerIdentifiers(cell: string): string[] {
  const names: string[] = [];

  BACKTICKED_SPAN_RE.lastIndex = 0;
  for (let m = BACKTICKED_SPAN_RE.exec(cell); m !== null; m = BACKTICKED_SPAN_RE.exec(cell)) {
    const span = m[1].trim();

    if (span.startsWith(BRIDGE_PREFIX)) {
      const name = span.slice(BRIDGE_PREFIX.length).replace(/\(\)$/, '');
      if (IDENTIFIER_RE.test(name) && !names.includes(name)) names.push(name);
      continue;
    }

    if (LOWER_INITIAL_IDENTIFIER_RE.test(span) && !names.includes(span)) names.push(span);
  }

  return names;
}

/**
 * The literal Consumer cell meaning "same as the row above".
 *
 * Resolved by inheritance rather than allowlisted. Rows 34-41 of
 * docs/gui-event-channels.md — the eight `claude-*` channels following
 * `claude-text-delta` — write `same` in their Producer, Consumer AND Notes
 * cells, and they genuinely do share `subscribeToClaudeEvents`. Allowlisting
 * them would add eight entries that all say "excused", diluting an allowlist
 * whose signal is meant to be "this row has an unusual NON-bridge consumer";
 * bridgeMockCoverage.test.ts documents exactly that failure mode when it
 * refuses to glob its target list. Resolving instead leaves those eight rows
 * genuinely checked against a real export.
 */
const INHERIT_SENTINEL = 'same';

/**
 * The literal Consumer cell meaning "this channel deliberately has no consumer".
 *
 * Recognised for two reasons. A channel documented as consumer-less is a
 * legitimate recurring state that should not need an ALLOWLIST entry naming an
 * absence — the allowlist's signal is "this row has an unusual non-bridge
 * consumer", which is a different claim. And it decouples this guard from task
 * 6227's merge order: 6227 deletes `bridge.ts::onDiagnostics` and sets the
 * `diagnostics` row's Consumer to `*(none)*`, so without this sentinel whichever
 * of the two branches landed second would turn the other's tree RED and force a
 * cross-task allowlist edit. With it, both merge orders are green.
 *
 * Recognised is NOT unaudited: an `explicit-none` row still has to be named in
 * the consumer suite's deliberately-consumer-less register, or
 * `unregisteredConsumerlessRows` fails it. Otherwise this sentinel would be a
 * weaker escape hatch than the allowlist it exists to keep uncluttered.
 */
const EXPLICIT_NONE_SENTINEL = '*(none)*';

/**
 * How a row's Consumer cell was resolved.
 *
 *  - `named`           — the cell itself yielded bridge identifiers.
 *  - `inherited`       — the cell is `same`; identifiers come from the nearest
 *                        preceding `named` row in the same section.
 *  - `explicit-none`   — the cell is `*(none)*`; no consumer, by assertion.
 *  - `needs-allowlist` — everything else. The SAFE DEFAULT: any cell this
 *                        parser cannot resolve to bridge identifiers demands a
 *                        deliberate allowlist entry carrying a prose reason, so
 *                        an unparsed row FAILS the guard rather than silently
 *                        dropping out of the checked set.
 */
export type ConsumerRowKind = 'named' | 'inherited' | 'explicit-none' | 'needs-allowlist';

/** A parsed row plus the resolution of its Consumer cell. */
export interface ClassifiedRow {
  section: EventChannelRow['section'];
  channel: string;
  /** Bridge identifiers this row claims — its own, or inherited. Never partial. */
  identifiers: string[];
  kind: ConsumerRowKind;
}

/**
 * Resolve every parsed row's Consumer cell, in document order.
 *
 * A single forward walk tracking, PER SECTION, the resolution of the IMMEDIATELY
 * PRECEDING row. Two properties of that tracker are deliberate and pinned by
 * unit tests:
 *
 *  - It is keyed by section, so inheritance never crosses the §1/§2 boundary.
 *    §2 is a separate table with its own authorship; a `same` at the top of §2
 *    does not mean "same as the last §1 row", and inheriting there would invent
 *    a consumer nobody wrote.
 *  - It tracks the previous row WHATEVER its kind, not the last row that
 *    happened to name a consumer. `same` means literally "same as the row
 *    above", so a `same` below an `*(none)*` row resolves to `explicit-none` and
 *    a `same` below an unresolved row stays `needs-allowlist`. Skipping over the
 *    predecessor to a `named` row two-or-more positions up would hand the row a
 *    consumer nobody wrote for it, and — because that inherited name is a real
 *    export — it would then pass BOTH `unknownConsumers` and `uncoveredRows`,
 *    reporting as guarded while guarding the wrong symbol. That false green is
 *    the one failure direction this module is built to exclude.
 *
 * Degradation is safe rather than clever: a `same` with no predecessor in its
 * section yields `needs-allowlist` with no identifiers, NOT a throw. Any
 * spelling other than the two exact sentinels falls through to `needs-allowlist`
 * too, so a variant like `Same` or `(none)` cannot silently excuse a row.
 */
export function classifyEventChannelRows(rows: EventChannelRow[]): ClassifiedRow[] {
  const previousBySection = new Map<EventChannelRow['section'], ClassifiedRow>();
  const resolved: ClassifiedRow[] = [];

  for (const { section, channel, consumerCell } of rows) {
    const own = extractConsumerIdentifiers(consumerCell);
    const cell = consumerCell.trim();
    let row: ClassifiedRow;

    if (own.length > 0) {
      row = { section, channel, identifiers: own, kind: 'named' };
    } else if (cell === INHERIT_SENTINEL) {
      const previous = previousBySection.get(section);
      if (previous === undefined) {
        row = { section, channel, identifiers: [], kind: 'needs-allowlist' };
      } else if (previous.kind === 'named' || previous.kind === 'inherited') {
        // Chains: a run of `same` rows all resolve to the identifiers of the
        // last row that spelled them out, one hop at a time.
        row = { section, channel, identifiers: [...previous.identifiers], kind: 'inherited' };
      } else {
        // `explicit-none` and `needs-allowlist` propagate verbatim: "same as
        // above" of an asserted absence is an asserted absence, and of an
        // unresolved cell is still unresolved. Neither carries identifiers, so
        // an inheriting row is held to exactly the accountability its
        // predecessor is — a register entry or an allowlist entry.
        row = { section, channel, identifiers: [], kind: previous.kind };
      }
    } else if (cell === EXPLICIT_NONE_SENTINEL) {
      row = { section, channel, identifiers: [], kind: 'explicit-none' };
    } else {
      row = { section, channel, identifiers: [], kind: 'needs-allowlist' };
    }

    previousBySection.set(section, row);
    resolved.push(row);
  }

  return resolved;
}

/**
 * One doc row naming a consumer that `gui/src/bridge.ts` does not export.
 *
 * A `{channel, name}` PAIR rather than a bare name, so a failure message points
 * at the doc row a maintainer has to edit — the symbol alone would leave them
 * grepping. One symbol named by two rows is therefore two findings.
 */
export interface UnknownConsumer {
  channel: string;
  name: string;
}

/** Compare two findings by channel, then by name. Keeps failure output stable. */
function byChannelThenName(a: UnknownConsumer, b: UnknownConsumer): number {
  return a.channel === b.channel ? a.name.localeCompare(b.name) : a.channel.localeCompare(b.channel);
}

/**
 * THE GUARD: every bridge-shaped consumer the doc names that `bridge.ts` does
 * not actually export, sorted.
 *
 * `inherited` identifiers are checked exactly like `named` ones — that is the
 * point of resolving `same` rather than allowlisting it. Deleting an export
 * named by an inheritance source implicates every row that inherits from it,
 * not just the one that spells it out.
 *
 * `needs-allowlist` and `explicit-none` rows carry no identifiers, so they
 * contribute nothing here; their coverage is `uncoveredRows`' job.
 */
export function unknownConsumers(
  runtimeExports: string[],
  rows: ClassifiedRow[],
): UnknownConsumer[] {
  const exported = new Set(runtimeExports);
  return rows
    .flatMap(({ channel, identifiers }) =>
      identifiers.filter((name) => !exported.has(name)).map((name) => ({ channel, name })),
    )
    .sort(byChannelThenName);
}

/**
 * Channels of every row of `kind` that has no entry in `register`, sorted.
 *
 * `Object.hasOwn`, never the `in` operator: `in` walks the prototype chain, and
 * these registers are plain object literals, so `in` would report a channel
 * named `constructor` as registered against `Object.prototype.constructor` with
 * nobody having written an entry. `CHANNEL_ROW_RE`'s `[a-z0-9-]+` name class
 * rules out `toString`/`valueOf`/`hasOwnProperty`, but `constructor` matches it
 * exactly — and this is the one place where a language detail could defeat the
 * safe-default discipline the rest of the module is built on.
 */
function channelsMissingFromRegister(
  rows: ClassifiedRow[],
  kind: ConsumerRowKind,
  register: Record<string, string>,
): string[] {
  return rows
    .filter((r) => r.kind === kind && !Object.hasOwn(register, r.channel))
    .map((r) => r.channel)
    .sort();
}

/**
 * THE NON-VACUITY FLOOR: rows this parser could not resolve to a consumer and
 * which carry no allowlist entry either, sorted.
 *
 * Row-granular on purpose. A global `>= N` count floor is weak — a parser that
 * drops 5 of 40 rows still clears it, and the dropped rows are exactly the ones
 * that stopped being guarded. Requiring every row to land in {named, inherited,
 * explicit-none, allowlisted} instead means a parse miss surfaces HERE and
 * fails, rather than silently shrinking the checked set.
 */
export function uncoveredRows(rows: ClassifiedRow[], allowlist: Record<string, string>): string[] {
  return channelsMissingFromRegister(rows, 'needs-allowlist', allowlist);
}

/**
 * THE `*(none)*` ACCOUNTABILITY CHECK: `explicit-none` rows with no entry in the
 * deliberately-consumer-less register, sorted.
 *
 * Without this, `*(none)*` would be a strictly weaker escape hatch than the
 * allowlist it sits beside: an `explicit-none` row contributes nothing to
 * `unknownConsumers`, is excluded from `uncoveredRows`, and is invisible to
 * `staleAllowlistEntries`. A maintainer wanting to silence this guard for a row
 * could simply write `*(none)*` in the Consumer cell — no reason, no self-check,
 * no reviewer signal — and, because a docs-only edit does not run the gui suite,
 * do it without ever tripping the gate. Requiring a register entry puts an
 * `*(none)*` row behind the same reviewed, prose-carrying edit the allowlist
 * demands, and restores the "every row lands in one of four accounted-for
 * buckets" claim the consumer suite makes.
 *
 * The register is a PRE-COMMITMENT list, not a post-hoc excuse list — see
 * `staleConsumerlessEntries` for why that direction is the one that works.
 */
export function unregisteredConsumerlessRows(
  rows: ClassifiedRow[],
  register: Record<string, string>,
): string[] {
  return channelsMissingFromRegister(rows, 'explicit-none', register);
}

/**
 * THE ALLOWLIST SELF-CHECK: entries that have rotted, sorted — either because
 * no parsed row carries that channel any more, or because the row it names is
 * no longer `needs-allowlist` (it now yields a bridge consumer of its own, or
 * inherits one, or declares an explicit absence).
 *
 * Without this the allowlist decays into a rubber stamp: entries whose stated
 * reason stopped being true keep suppressing checks nobody re-reads. Both rot
 * directions matter — a renamed channel leaves an excuse protecting nothing,
 * and a newly-wired channel keeps an excuse that now hides a checkable row.
 */
export function staleAllowlistEntries(
  rows: ClassifiedRow[],
  allowlist: Record<string, string>,
): string[] {
  const needing = new Set(rows.filter((r) => r.kind === 'needs-allowlist').map((r) => r.channel));
  return Object.keys(allowlist)
    .filter((channel) => !needing.has(channel))
    .sort();
}

/**
 * THE CONSUMER-LESS REGISTER SELF-CHECK: entries naming a channel that no §1/§2
 * row carries any more, sorted.
 *
 * Deliberately ONE-directional, unlike `staleAllowlistEntries` — an entry whose
 * row is not (yet) `explicit-none` is NOT reported, and that asymmetry is the
 * whole reason the register works:
 *
 *  - The register SUPPRESSES NOTHING. What excuses an `explicit-none` row is the
 *    doc cell itself; the entry is a co-signature that forces the doc edit to be
 *    accompanied by a reviewed, reasoned edit here. So an entry that has gone
 *    ahead of (or behind) its row hides no checkable row — it is dead weight,
 *    not a hole. `staleAllowlistEntries` must flag both directions precisely
 *    because an allowlist entry DOES suppress a check.
 *  - Pre-registration is therefore the correct usage, and the only usage that
 *    keeps this guard decoupled from another task's merge order. A task that
 *    turns a row's Consumer cell into `*(none)*` (task 6227 does exactly this to
 *    the `diagnostics` row) holds no lock on this file; requiring it to add an
 *    entry AS it lands would turn a cross-task doc edit into a red tree here.
 *    Writing the entry ahead of time — with the reason and the task number —
 *    is the review, and it lands in whichever order.
 *
 * A key naming no parsed row at all is still rot worth reporting: the channel
 * was renamed or deleted, so the entry now documents nothing.
 */
export function staleConsumerlessEntries(
  rows: ClassifiedRow[],
  register: Record<string, string>,
): string[] {
  const parsed = new Set(rows.map((r) => r.channel));
  return Object.keys(register)
    .filter((channel) => !parsed.has(channel))
    .sort();
}

/**
 * THE CODE→DOC PIN'S ITERATION SET: the register entries whose row has actually
 * LANDED as `*(none)*` — `Object.keys(register)` ∩ the channels of every
 * `explicit-none` row — sorted. This is the set check (f) of
 * `eventChannelConsumerCoverage.test.ts` iterates.
 *
 * The INTERSECTION, not the register itself, and that is the point. An entry
 * whose row has not yet flipped to `*(none)*` pins nothing and suppresses
 * nothing: the doc cell is what asserts the absence, so until it lands there is
 * no documented absence for bridge.ts to contradict. Asserting against such an
 * entry anyway would re-couple this suite to another task's merge order —
 * precisely what the pre-registration contract documented twice here
 * (`staleConsumerlessEntries` above, and `DELIBERATELY_CONSUMERLESS`'s docblock
 * in the coverage suite) exists to prevent. Defined over register KEYS rather
 * than over rows for the same reason (f) is: an entry is what carries the
 * reviewed reason that makes a code→doc pin owed in the first place.
 *
 * Third member of the register/row trio, and it owns only the intersection —
 * both rot directions belong to its neighbours, so (f) never re-reports what
 * check (e) already covers:
 *
 *  - `unregisteredConsumerlessRows` — row with no entry.
 *  - `staleConsumerlessEntries`     — entry with no row.
 *  - `landedConsumerlessChannels`   — entry whose row landed. THIS ONE.
 */
export function landedConsumerlessChannels(
  rows: ClassifiedRow[],
  register: Record<string, string>,
): string[] {
  const landed = new Set(rows.filter((r) => r.kind === 'explicit-none').map((r) => r.channel));
  return Object.keys(register)
    .filter((channel) => landed.has(channel))
    .sort();
}

/**
 * Every place `source` names `channel` in a registration position, covering
 * both shapes `gui/src/bridge.ts` uses: the direct `listen<T>('<channel>', ...)`
 * call (~30 sites) and the `['<channel>', mapper]` tuple entries
 * `subscribeToClaudeEvents` feeds to a loop over `listen(name, mapper)`
 * (bridge.ts:457) — a bare `listen('` match would miss the second entirely.
 * Requiring a `(` or `[` immediately before the quote keeps prose and bare
 * identifiers out.
 *
 * Pure over a caller-supplied `source` string, like the rest of this module —
 * it does not itself read bridge.ts off disk, so a caller can pass any source
 * text (real or synthetic-for-a-unit-test).
 *
 * Originated as a bespoke helper in `bridge.test.ts` (task 6227, pinning the
 * `diagnostics` channel only); lifted here (task 6380) so
 * `eventChannelConsumerCoverage.test.ts` can run the same matcher over every
 * channel in `DELIBERATELY_CONSUMERLESS`, not just one hardcoded name.
 */
export function channelRegistrationsIn(source: string, channel: string): string[] {
  const pattern = new RegExp(`[([]\\s*(['"\`])${channel}\\1`, 'g');
  return [...source.matchAll(pattern)].map((m) => m[0]);
}
