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
 * Spec` — so one index serves both and no per-section header parse is needed.
 */
const CONSUMER_CELL_INDEX = 4;

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
 */
export function parseEventChannelRows(markdown: string): EventChannelRow[] {
  const rows: EventChannelRow[] = [];
  let section: EventChannelRow['section'] | null = null;

  for (const line of markdown.split('\n')) {
    if (line.startsWith('### ')) {
      section = null;
      continue;
    }
    if (line.startsWith('## ')) {
      section = sectionOf(line);
      continue;
    }
    if (section === null) continue;

    const match = CHANNEL_ROW_RE.exec(line);
    if (match === null) continue;

    const cells = splitTableCells(line);
    rows.push({
      section,
      channel: match[1],
      consumerCell: cells[CONSUMER_CELL_INDEX] ?? '',
    });
  }

  return rows;
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
 * legitimate recurring state that should not need an allowlist entry naming an
 * absence. And it decouples this guard from task 6227's merge order: 6227
 * deletes `bridge.ts::onDiagnostics` and sets the `diagnostics` row's Consumer
 * to `*(none)*`, so without this sentinel whichever of the two branches landed
 * second would turn the other's tree RED and force a cross-task allowlist edit.
 * With it, both merge orders are green with no coordination.
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
 * A single forward walk tracking, PER SECTION, the identifiers of the last row
 * whose own cell yielded any. Two properties of that tracker are deliberate and
 * pinned by unit tests:
 *
 *  - It is keyed by section, so inheritance never crosses the §1/§2 boundary.
 *    §2 is a separate table with its own authorship; a `same` at the top of §2
 *    does not mean "same as the last §1 row", and inheriting there would invent
 *    a consumer nobody wrote.
 *  - It only advances on a `named` row, so an `*(none)*` row cannot become an
 *    inheritance source — it asserts an absence, and there is nothing to inherit.
 *
 * Degradation is safe rather than clever: a `same` with no eligible predecessor
 * yields `needs-allowlist` with no identifiers, NOT a throw. Any spelling other
 * than the two exact sentinels falls through to `needs-allowlist` too, so a
 * variant like `Same` or `(none)` cannot silently excuse a row.
 */
export function classifyEventChannelRows(rows: EventChannelRow[]): ClassifiedRow[] {
  const lastNamedBySection = new Map<EventChannelRow['section'], string[]>();

  return rows.map(({ section, channel, consumerCell }) => {
    const own = extractConsumerIdentifiers(consumerCell);
    if (own.length > 0) {
      lastNamedBySection.set(section, own);
      return { section, channel, identifiers: own, kind: 'named' };
    }

    const cell = consumerCell.trim();

    if (cell === INHERIT_SENTINEL) {
      const inherited = lastNamedBySection.get(section);
      if (inherited !== undefined) {
        return { section, channel, identifiers: [...inherited], kind: 'inherited' };
      }
      return { section, channel, identifiers: [], kind: 'needs-allowlist' };
    }

    if (cell === EXPLICIT_NONE_SENTINEL) {
      return { section, channel, identifiers: [], kind: 'explicit-none' };
    }

    return { section, channel, identifiers: [], kind: 'needs-allowlist' };
  });
}
