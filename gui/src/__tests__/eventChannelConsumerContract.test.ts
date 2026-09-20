/**
 * Unit tests for the pure helpers in `eventChannelConsumerContract` — the
 * markdown-parsing and set-difference logic behind the event-channel Consumer
 * column guard (`eventChannelConsumerCoverage.test.ts`, task 6236).
 *
 * Synthetic string/array input only: no filesystem, no vitest module mocks, no
 * `../bridge` import. Two reasons this file exists rather than testing the live
 * guard alone:
 *
 *  1. Markdown-table parsing is the one fragile component of the guard. Keeping
 *     it a pure function over strings lets the escaped-pipe row, the `same`
 *     sentinel, section scoping and the noise-rejection rule be pinned directly
 *     from string literals — including forms that are not present in
 *     docs/gui-event-channels.md today and so could never be covered by the
 *     live suite.
 *  2. "The detector actually fires" is asserted MECHANICALLY here, by replaying
 *     the defect it exists to prevent (task 6227 deleting `bridge.ts::onDiagnostics`
 *     while the `diagnostics` row keeps naming it), rather than being claimed in
 *     a comment. Same discipline as `bridgeMockContract.test.ts`.
 */
import { describe, it, expect } from 'vitest';
import {
  CHANNEL_ROW_SCAN_RE,
  splitTableCells,
  tableDataRows,
  eventChannelTableHeaders,
  parseEventChannelRows,
  extractConsumerIdentifiers,
  classifyEventChannelRows,
  unknownConsumers,
  uncoveredRows,
  staleAllowlistEntries,
  unregisteredConsumerlessRows,
  staleConsumerlessEntries,
  landedConsumerlessChannels,
  channelRegistrationsIn,
  type ClassifiedRow,
  type EventChannelRow,
} from './eventChannelConsumerContract';

/** Terse `EventChannelRow` literal, so the classification tables stay readable. */
function row(section: EventChannelRow['section'], channel: string, consumerCell: string): EventChannelRow {
  return { section, channel, consumerCell };
}

/**
 * Terse `ClassifiedRow` literal for the set-difference tests, which take
 * already-classified rows so they can be exercised without re-deriving them
 * through the parser.
 */
function classified(
  channel: string,
  identifiers: string[],
  kind: ClassifiedRow['kind'],
  section: ClassifiedRow['section'] = '§1',
): ClassifiedRow {
  return { section, channel, identifiers, kind };
}

describe('splitTableCells', () => {
  it('splits a plain §1 row so the Consumer cell lands at index 4', () => {
    const row = '| `mesh-update` | `MeshData` (per-entity) | `delta_to_events` | `bridge.ts::onMeshUpdate` | Per-entity delta |';
    const cells = splitTableCells(row);

    // Index 0 is the empty span before the leading pipe, so the Nth data cell
    // is at index N. Consumer is the 4th data cell in BOTH §1 and §2.
    expect(cells[0]).toBe('');
    expect(cells[1]).toBe('`mesh-update`');
    expect(cells[4]).toBe('`bridge.ts::onMeshUpdate`');
  });

  it('treats a backslash-escaped pipe as literal cell content, keeping the Consumer at index 4', () => {
    // Shape of the real `warm-pool-event` row (docs/gui-event-channels.md:52):
    // its Payload cell carries `\|` inside a Rust-ish type literal. A naive
    // `split('|')` / `awk -F'|'` shifts every later column by one and reads the
    // PRODUCER cell as the Consumer — silently dropping `onWarmPoolEvent` from
    // coverage while the guard still reports green.
    const row =
      "| `warm-pool-event` | `WarmPoolEvent {kind: 'evicted'\\|'donated', size_bytes: u64}` | `TauriWarmPoolEventEmitter::emit` | `bridge.ts::onWarmPoolEvent` → `WarmPoolDebugPanel` | M-010 | Phase 3 | spec |";
    const cells = splitTableCells(row);

    expect(cells[4]).toBe('`bridge.ts::onWarmPoolEvent` → `WarmPoolDebugPanel`');
    // The escape is resolved to the literal pipe the markdown renders, and the
    // escaped pipe does NOT open a new cell.
    expect(cells[2]).toBe("`WarmPoolEvent {kind: 'evicted'|'donated', size_bytes: u64}`");

    // Cell count is stable against the same row written without the escape.
    const unescaped = row.replace("'evicted'\\|'donated'", "'evictedOrDonated'");
    expect(cells.length).toBe(splitTableCells(unescaped).length);
  });

  it('trims surrounding whitespace from each cell', () => {
    expect(splitTableCells('|   a   |  b |')).toStrictEqual(['', 'a', 'b', '']);
  });

  it('returns a single cell for a line with no pipes at all', () => {
    expect(splitTableCells('just prose')).toStrictEqual(['just prose']);
  });
});

describe('CHANNEL_ROW_SCAN_RE', () => {
  // The published grep contract from docs/gui-event-channels.md §preamble, in
  // its unanchored (awk) form — the same notion of "channel row" that
  // scripts/check_event_inventory.sh scans with.

  it('matches a backticked kebab-case first column', () => {
    expect(CHANNEL_ROW_SCAN_RE.test('| `mesh-update` | `MeshData` | p | c |')).toBe(true);
    expect(CHANNEL_ROW_SCAN_RE.test('| `warm-pool-event` | x | y | z |')).toBe(true);
  });

  it('rejects header rows, separators and prose', () => {
    expect(CHANNEL_ROW_SCAN_RE.test('| Channel | Payload | Producer | Consumer | Notes |')).toBe(false);
    expect(CHANNEL_ROW_SCAN_RE.test('|---|---|---|---|---|')).toBe(false);
    expect(CHANNEL_ROW_SCAN_RE.test('Prose mentioning `mesh-update` in passing.')).toBe(false);
  });

  it('rejects the forms deliberately outside the grep contract', () => {
    // §2a bold-first-column commands and §3 snake_case RPC names. Both are
    // excluded BY CONSTRUCTION per the doc, which is what lets this pattern and
    // the section walk agree on one row set.
    expect(CHANNEL_ROW_SCAN_RE.test('| **solver-cancel-request** | `{run_id}` | f → b |')).toBe(false);
    expect(CHANNEL_ROW_SCAN_RE.test('| `morph_stats` | `()` | `MorphStats` | accessor |')).toBe(false);
    expect(CHANNEL_ROW_SCAN_RE.test('| `Mesh-Update` | x | y |')).toBe(false);
  });
});

describe('tableDataRows', () => {
  // The name-class-INDEPENDENT recount. It exists because an "independent"
  // count that reused the `[a-z0-9-]+` name class would be blind in exactly the
  // same place the parser is: a row the class fails to recognise is missed by
  // both, the equality still holds, and the row goes silently unguarded.

  const TABLE = [
    'Some prose.',
    '',
    '| Channel | Payload | Consumer |',
    '|---|---|---|',
    '| `mesh-update` | `MeshData` | `bridge.ts::onMeshUpdate` |',
    '| `mesh_update_v2` | `MeshData` | `bridge.ts::onMeshUpdateV2` |',
    '| **solver-cancel-request** | `{run_id}` | n/a |',
    '',
  ].join('\n');

  it('returns data rows only — not the header, the separator, or prose', () => {
    expect(tableDataRows(TABLE)).toStrictEqual([
      '| `mesh-update` | `MeshData` | `bridge.ts::onMeshUpdate` |',
      '| `mesh_update_v2` | `MeshData` | `bridge.ts::onMeshUpdateV2` |',
      '| **solver-cancel-request** | `{run_id}` | n/a |',
    ]);
  });

  it('counts rows the channel-name class does NOT recognise', () => {
    // The whole point. `mesh_update_v2` (snake_case) and the bold-first-column
    // command row are invisible to CHANNEL_ROW_SCAN_RE, so a recount built on
    // that pattern would agree with a parser that dropped them. This one does
    // not: it sees 3 where the grep contract sees 1.
    const grepContractCount = TABLE.split('\n').filter((l) => CHANNEL_ROW_SCAN_RE.test(l)).length;
    expect(grepContractCount).toBe(1);
    expect(tableDataRows(TABLE).length).toBe(3);
  });

  it('handles several tables in one document', () => {
    const two = ['| A | B |', '|---|---|', '| 1 | 2 |', '', '| C | D |', '|---|---|', '| 3 | 4 |'].join('\n');
    expect(tableDataRows(two)).toStrictEqual(['| 1 | 2 |', '| 3 | 4 |']);
  });

  it('returns [] for markdown with no tables', () => {
    expect(tableDataRows('# Title\n\nJust prose.\n')).toStrictEqual([]);
  });
});

describe('parseEventChannelRows', () => {
  // A miniature inventory carrying every structural form the real doc has: §1
  // and §2 channel rows, a §2a bold-first-column command row, a §3 snake_case
  // RPC row, §4/§5 rows, headings, separators and prose.
  const DOC = [
    '# GUI Event Channel Inventory',
    '',
    'Prose mentioning `mesh-update` in passing.',
    '',
    '## §1 — Wired channels (production today)',
    '',
    '| Channel | Payload | Producer | Consumer | Notes |',
    '|---|---|---|---|---|',
    '| `mesh-update` | `MeshData` | `delta_to_events` | `bridge.ts::onMeshUpdate` | delta |',
    '| `claude-done` | `{id}` | same | same | same |',
    '',
    '## §2 — Channels this PRD adds',
    '',
    '| Channel | Payload | Producer | Consumer | Upstream prereq | Owning slice | Spec |',
    '|---|---|---|---|---|---|---|',
    '| `auto-resolve-start` | `()` | `emit_auto_resolve_if_any` | `bridge.ts::onAutoResolveStart` → `AutoResolvePanel` | C-05 | Phase 2 | spec |',
    '',
    '### §2a — Tauri commands (not events; lint-exempt)',
    '',
    '| Command | Payload | Direction | Backend handler | Upstream prereq | Owning slice |',
    '|---|---|---|---|---|---|',
    '| **solver-cancel-request** | `{run_id}` | frontend → backend | `cancel_solve_impl` | task 2923 | Phase 3 |',
    '',
    '## §3 — Debug-MCP RPCs (snake_case; outside the §1/§2 grep contract)',
    '',
    '| RPC | Request shape | Response shape | Producer | Consumer | Upstream prereq |',
    '|---|---|---|---|---|---|',
    '| `morph_stats` | `()` | `MorphStats` | stats accessor | `mcp__reify-debug__morph_stats` | task 2949 |',
    '',
    '## §4 — Out-of-scope: payload extensions to existing channels',
    '',
    '| Extension | Existing channel | Owning PRD |',
    '|---|---|---|',
    '| `MeshData.thickness` per-vertex channel | `mesh-update` | `varying-thickness-shells.md` |',
    '',
    '## §5 — Out-of-scope: pure-frontend state',
    '',
    '- Display-mode toggle — Solid store in `gui/src/stores/`.',
    '',
  ].join('\n');

  const rows = parseEventChannelRows(DOC);

  it('returns exactly the §1 and §2 channel rows, tagged by section', () => {
    expect(rows.map((r) => [r.section, r.channel])).toStrictEqual([
      ['§1', 'mesh-update'],
      ['§1', 'claude-done'],
      ['§2', 'auto-resolve-start'],
    ]);
  });

  it('strips the backticks from the channel name', () => {
    // The awk in scripts/check_event_inventory.sh does the same, so both
    // readers of this doc agree on what a channel is NAMED, not just on which
    // lines are rows.
    for (const row of rows) {
      expect(row.channel).not.toContain('`');
    }
  });

  it('carries the 4th data cell as consumerCell for both §1 and §2 shapes', () => {
    expect(rows[0].consumerCell).toBe('`bridge.ts::onMeshUpdate`');
    expect(rows[1].consumerCell).toBe('same');
    expect(rows[2].consumerCell).toBe('`bridge.ts::onAutoResolveStart` → `AutoResolvePanel`');
  });

  it('excludes a §2a bold-first-column command row', () => {
    // Excluded twice over: the section walk resets on any `### ` heading, and
    // `**solver-cancel-request**` is outside the backtick grep contract by
    // construction (docs/gui-event-channels.md §2a says so normatively).
    expect(rows.map((r) => r.channel)).not.toContain('solver-cancel-request');
  });

  it('excludes §3 snake_case RPC rows and §4/§5 content', () => {
    const channels = rows.map((r) => r.channel);
    expect(channels).not.toContain('morph_stats');
    // §4's rows name `mesh-update` in their SECOND column; nothing outside
    // §1/§2 may contribute a row, or §4 would re-register §1 channels.
    expect(channels.filter((c) => c === 'mesh-update').length).toBe(1);
  });

  it('ignores headings, separator rows, header rows and prose', () => {
    // The `|---|---|` separator and the `| Channel | Payload | …` header row
    // are both inside §1 but match neither the backtick contract nor the
    // kebab-case name class.
    expect(rows.map((r) => r.channel)).not.toContain('Channel');
    expect(rows.length).toBe(3);
  });

  it('returns [] for an empty document rather than throwing', () => {
    expect(parseEventChannelRows('')).toStrictEqual([]);
  });

  it('returns [] for a table that appears before any §-heading', () => {
    const orphan = '| `mesh-update` | `MeshData` | `delta_to_events` | `bridge.ts::onMeshUpdate` | |';
    expect(parseEventChannelRows(orphan)).toStrictEqual([]);
  });

  it('refuses a table whose header does not put Consumer at the read index', () => {
    // `CONSUMER_CELL_INDEX` is a positional assumption, so it is CHECKED rather
    // than assumed. Here an `Owner` column is inserted ahead of Consumer, which
    // shifts the Producer cell into the read position. Reading it would be
    // silently wrong — and a producer written as a bare lowercase identifier
    // (`emitStatus` below) is bridge-SHAPED, so it would sail through the
    // extraction rule as if it were the consumer. Fail closed instead.
    const shifted = [
      '## §1 — Wired channels (production today)',
      '',
      '| Channel | Payload | Owner | Producer | Consumer | Notes |',
      '|---|---|---|---|---|---|',
      '| `evaluation-status` | `{}` | gui | `emitStatus` | `bridge.ts::onEvaluationStatus` | |',
    ].join('\n');

    expect(parseEventChannelRows(shifted)).toStrictEqual([]);
  });

  it('refuses channel rows in a section with no header/separator above them', () => {
    const headerless = [
      '## §1 — Wired channels (production today)',
      '',
      '| `mesh-update` | `MeshData` | `delta_to_events` | `bridge.ts::onMeshUpdate` | |',
    ].join('\n');

    expect(parseEventChannelRows(headerless)).toStrictEqual([]);
  });

  it('re-validates the header per table, so §2 is not trusted because §1 was', () => {
    // Confirmation must not leak across a heading: §1 is well-formed here, §2 is
    // not, and only §1's row may survive.
    const mixed = [
      '## §1 — Wired',
      '| Channel | Payload | Producer | Consumer | Notes |',
      '|---|---|---|---|---|',
      '| `mesh-update` | `MeshData` | `delta_to_events` | `bridge.ts::onMeshUpdate` | |',
      '',
      '## §2 — Added',
      '| Channel | Payload | Producer | Owner | Consumer |',
      '|---|---|---|---|---|',
      '| `mode-shape-frame` | `{}` | solver | gui | `bridge.ts::onModeShapeFrame` |',
    ].join('\n');

    expect(parseEventChannelRows(mixed).map((r) => r.channel)).toStrictEqual(['mesh-update']);
  });
});

describe('eventChannelTableHeaders', () => {
  // The legible half of the same positional check. `parseEventChannelRows` fails
  // CLOSED on a bad header, which shows up downstream as "expected 0 to be 40";
  // this lets the consumer suite name the header it actually found.

  const HEADERS_DOC = [
    '## §1 — Wired',
    '| Channel | Payload | Producer | Consumer | Notes |',
    '|---|---|---|---|---|',
    '| `mesh-update` | `MeshData` | `delta_to_events` | `bridge.ts::onMeshUpdate` | |',
    '',
    '## §2 — Added',
    '| Channel | Payload | Producer | Consumer | Upstream prereq | Owning slice | Spec |',
    '|---|---|---|---|---|---|---|',
    '| `mode-shape-frame` | `{}` | solver | (new) `BucklingPanel` animator | GR-024 | Phase 9 | spec |',
    '',
    '### §2a — Tauri commands (lint-exempt)',
    '| Command | Payload | Direction | Backend handler |',
    '|---|---|---|---|',
    '| **solver-cancel-request** | `{run_id}` | frontend → backend | `cancel_solve_impl` |',
    '',
    '## §3 — Debug-MCP RPCs',
    '| RPC | Request shape | Response shape | Producer | Consumer | Upstream prereq |',
    '|---|---|---|---|---|---|',
    '| `morph_stats` | `()` | `MorphStats` | accessor | `mcp__reify-debug__morph_stats` | task 2949 |',
  ].join('\n');

  it('returns the header cells of each §1/§2 table, tagged by section', () => {
    const headers = eventChannelTableHeaders(HEADERS_DOC);

    expect(headers.map((h) => h.section)).toStrictEqual(['§1', '§2']);
    expect(headers[0].cells[4]).toBe('Consumer');
    expect(headers[1].cells[4]).toBe('Consumer');
  });

  it('ignores tables outside §1/§2, including §2a and §3', () => {
    // §3's header ALSO has `Consumer` at index 4, so returning it would let a
    // consumer-suite assertion over these headers pass on the strength of a
    // table this guard does not read.
    expect(eventChannelTableHeaders(HEADERS_DOC).length).toBe(2);
  });

  it('returns [] for markdown with no §1/§2 table', () => {
    expect(eventChannelTableHeaders('# Title\n\nProse.\n')).toStrictEqual([]);
  });
});

describe('extractConsumerIdentifiers', () => {
  // ── Forms that ARE bridge consumers ────────────────────────────────────────

  it('takes the suffix of a `bridge.ts::NAME` span', () => {
    expect(extractConsumerIdentifiers('`bridge.ts::onMeshUpdate`')).toStrictEqual(['onMeshUpdate']);
  });

  it('takes a bare lowercase-initial backticked identifier', () => {
    expect(extractConsumerIdentifiers('`onMeshRemoved`')).toStrictEqual(['onMeshRemoved']);
  });

  it('takes a bare `subscribeTo…` identifier — these ARE bridge.ts exports', () => {
    // The 10 claude-* / sidecar rows name their consumer without the
    // `bridge.ts::` prefix. Both names are real runtime exports of bridge.ts,
    // so dropping them would silently shrink the guarded set by a fifth.
    expect(extractConsumerIdentifiers('`subscribeToClaudeEvents`')).toStrictEqual([
      'subscribeToClaudeEvents',
    ]);
    expect(extractConsumerIdentifiers('`subscribeToSidecarCrashed`')).toStrictEqual([
      'subscribeToSidecarCrashed',
    ]);
  });

  it('takes only the bridge identifier out of a multi-token routing cell', () => {
    // The live `fea-diagnostics-changed` row (docs/gui-event-channels.md:23)
    // documents the whole route, not just the bridge hop.
    expect(
      extractConsumerIdentifiers(
        '`bridge.ts::onFeaDiagnosticsChanged` → `engineStore.setFeaDiagnostics` → FeaDiagnosticsPanel + DualViewport overlay',
      ),
    ).toStrictEqual(['onFeaDiagnosticsChanged']);
  });

  it('strips a trailing () from a bridge.ts:: span', () => {
    expect(extractConsumerIdentifiers('`bridge.ts::cancelSolve()`')).toStrictEqual(['cancelSolve']);
  });

  it('deduplicates while preserving source order', () => {
    expect(
      extractConsumerIdentifiers('`bridge.ts::onSolverProgress` … also `onSolverProgress` … `onFocusEntity`'),
    ).toStrictEqual(['onSolverProgress', 'onFocusEntity']);
  });

  // ── Noise that MUST be rejected, one assertion per shape ───────────────────
  //
  // Each of these appears in a live Consumer cell today. Rejecting them
  // mechanically is what keeps the allowlist at 2 entries instead of ~10:
  // an allowlist bloated with entries that all say "not a bridge symbol" has
  // no signal left (the warning bridgeMockCoverage.test.ts makes explicitly).

  it('rejects a dotted store path', () => {
    expect(extractConsumerIdentifiers('`engineStore.setFeaConvergence`')).toStrictEqual([]);
  });

  it('rejects slashed paths', () => {
    expect(extractConsumerIdentifiers('`gui/src` debug-bridge')).toStrictEqual([]);
    expect(extractConsumerIdentifiers('`gui/src/debug/WarmPoolDebugPanel.tsx`')).toStrictEqual([]);
  });

  it('rejects PascalCase Solid component names', () => {
    for (const component of [
      'BucklingPanel',
      'AutoResolvePanel',
      'WarmPoolDebugPanel',
      'SolverProgressOverlay',
      'FeaCasePickerDropdown',
    ]) {
      expect(
        extractConsumerIdentifiers(`\`${component}\``),
        `${component} is a component, not a bridge export`,
      ).toStrictEqual([]);
    }
  });

  it('rejects unbackticked prose', () => {
    expect(extractConsumerIdentifiers('same')).toStrictEqual([]);
    expect(extractConsumerIdentifiers('(new) animator')).toStrictEqual([]);
    // Backticked component inside prose is still rejected, prose still ignored.
    expect(extractConsumerIdentifiers('(new) `BucklingPanel` animator')).toStrictEqual([]);
  });

  it('returns [] for a cell with no backticks at all', () => {
    expect(extractConsumerIdentifiers('')).toStrictEqual([]);
    expect(extractConsumerIdentifiers('*(none)*')).toStrictEqual([]);
  });
});

describe('classifyEventChannelRows', () => {
  it('classifies an identifier-bearing cell as named', () => {
    expect(
      classifyEventChannelRows([row('§1', 'mesh-update', '`bridge.ts::onMeshUpdate`')]),
    ).toStrictEqual([
      { section: '§1', channel: 'mesh-update', identifiers: ['onMeshUpdate'], kind: 'named' },
    ]);
  });

  it('resolves the literal `same` cell by inheriting the nearest preceding named row', () => {
    // Live shape: docs/gui-event-channels.md rows 34-41 write `same` in the
    // Producer, Consumer AND Notes cells of the 8 claude-* rows that follow
    // `claude-text-delta`, meaning "same as the row above" —
    // i.e. `subscribeToClaudeEvents`. Resolving that mechanically is what keeps
    // those 8 rows genuinely CHECKED against a real export, instead of adding 8
    // allowlist entries that would all say "excused" and dilute an allowlist
    // whose signal is meant to be "this row has an unusual non-bridge consumer".
    const claudeRows = [
      row('§1', 'claude-text-delta', '`subscribeToClaudeEvents`'),
      row('§1', 'claude-thinking-delta', 'same'),
      row('§1', 'claude-tool-call', 'same'),
      row('§1', 'claude-tool-result', 'same'),
      row('§1', 'claude-done', 'same'),
      row('§1', 'claude-error', 'same'),
      row('§1', 'claude-notice', 'same'),
      row('§1', 'claude-ready', 'same'),
      row('§1', 'claude-permission-request', 'same'),
    ];

    const classified = classifyEventChannelRows(claudeRows);

    expect(classified[0].kind).toBe('named');
    expect(classified.slice(1).map((c) => c.kind)).toStrictEqual(Array(8).fill('inherited'));
    for (const c of classified.slice(1)) {
      expect(c.identifiers, `${c.channel} should inherit the claude-text-delta consumer`).toStrictEqual([
        'subscribeToClaudeEvents',
      ]);
    }
  });

  it('inherits from the immediately preceding row, not the first one in the section', () => {
    const classified = classifyEventChannelRows([
      row('§1', 'a', '`onA`'),
      row('§1', 'b', '`onB`'),
      row('§1', 'c', 'same'),
    ]);

    expect(classified[2]).toStrictEqual({
      section: '§1',
      channel: 'c',
      identifiers: ['onB'],
      kind: 'inherited',
    });
  });

  it('degrades a `same` row with no preceding row to needs-allowlist rather than throwing', () => {
    const classified = classifyEventChannelRows([row('§1', 'orphan', 'same')]);

    expect(classified).toStrictEqual([
      { section: '§1', channel: 'orphan', identifiers: [], kind: 'needs-allowlist' },
    ]);
  });

  it('never inherits across the §1/§2 boundary', () => {
    // §2 is a separate table with its own Producer/Consumer authorship; a `same`
    // at the top of §2 does NOT mean "same as the last §1 row". Inheriting there
    // would invent a consumer for a row nobody wrote one for — so the §2 row
    // degrades to needs-allowlist and must be fixed or allowlisted deliberately.
    const classified = classifyEventChannelRows([
      row('§1', 'claude-text-delta', '`subscribeToClaudeEvents`'),
      row('§2', 'mode-shape-frame', 'same'),
    ]);

    expect(classified[1]).toStrictEqual({
      section: '§2',
      channel: 'mode-shape-frame',
      identifiers: [],
      kind: 'needs-allowlist',
    });
  });

  it('classifies the `*(none)*` sentinel as explicit-none with no identifiers', () => {
    expect(classifyEventChannelRows([row('§1', 'diagnostics', '*(none)*')])).toStrictEqual([
      { section: '§1', channel: 'diagnostics', identifiers: [], kind: 'explicit-none' },
    ]);
  });

  it('resolves a `same` below an `*(none)*` row to explicit-none, not to an earlier consumer', () => {
    // `same` means literally "same as the row above". Skipping over the
    // `*(none)*` predecessor to `onA` two rows up would hand `c` a consumer
    // nobody wrote for it — and because `onA` is a REAL export, the misattributed
    // row would then pass both `unknownConsumers` and `uncoveredRows` and report
    // as guarded while guarding the wrong symbol. Inheriting the absence instead
    // keeps `c` accountable to the deliberately-consumer-less register.
    const classified = classifyEventChannelRows([
      row('§1', 'a', '`onA`'),
      row('§1', 'diagnostics', '*(none)*'),
      row('§1', 'c', 'same'),
    ]);

    expect(classified.map((r) => [r.channel, r.kind, r.identifiers])).toStrictEqual([
      ['a', 'named', ['onA']],
      ['diagnostics', 'explicit-none', []],
      ['c', 'explicit-none', []],
    ]);
  });

  it('resolves a `same` below an unresolved row to needs-allowlist, not to an earlier consumer', () => {
    // Same rule, the other unresolved kind: inheriting "I could not be parsed"
    // keeps the row demanding a deliberate allowlist entry instead of silently
    // acquiring `onA`.
    const classified = classifyEventChannelRows([
      row('§1', 'a', '`onA`'),
      row('§1', 'debug-request', '`gui/src` debug-bridge'),
      row('§1', 'c', 'same'),
    ]);

    expect(classified.map((r) => [r.channel, r.kind, r.identifiers])).toStrictEqual([
      ['a', 'named', ['onA']],
      ['debug-request', 'needs-allowlist', []],
      ['c', 'needs-allowlist', []],
    ]);
  });

  it('chains `same` rows one hop at a time through an inherited predecessor', () => {
    // The live claude-* run relies on this: only the first row spells the
    // consumer out, and each `same` below it inherits from the `inherited` row
    // directly above rather than reaching back to the `named` one.
    const classified = classifyEventChannelRows([
      row('§1', 'a', '`onA`'),
      row('§1', 'b', 'same'),
      row('§1', 'c', 'same'),
      row('§1', 'd', 'same'),
    ]);

    expect(classified.map((r) => r.kind)).toStrictEqual(['named', 'inherited', 'inherited', 'inherited']);
    for (const r of classified) expect(r.identifiers).toStrictEqual(['onA']);
  });

  it('classifies any other identifier-free prose cell as needs-allowlist', () => {
    // Both live today: `debug-request` points at the gui/src debug-bridge
    // module, and `mode-shape-frame` at an unwired Solid component. Neither is a
    // bridge export, and neither uses a sentinel — so each must be explicitly
    // allowlisted by the consumer suite or the guard fails.
    const classified = classifyEventChannelRows([
      row('§1', 'debug-request', '`gui/src` debug-bridge'),
      row('§2', 'mode-shape-frame', '(new) `BucklingPanel` animator'),
    ]);

    expect(classified.map((c) => [c.channel, c.kind, c.identifiers])).toStrictEqual([
      ['debug-request', 'needs-allowlist', []],
      ['mode-shape-frame', 'needs-allowlist', []],
    ]);
  });

  it('treats a near-miss sentinel spelling as needs-allowlist, the safe default', () => {
    // Only the exact trimmed strings `same` and `*(none)*` are sentinels. A
    // variant spelling must NOT silently excuse a row — it falls through to the
    // classification that demands a deliberate allowlist entry.
    for (const cell of ['Same', 'same as above', '(none)', '*none*', '—', 'n/a']) {
      expect(
        classifyEventChannelRows([row('§1', 'x', cell)])[0].kind,
        `${cell} must not be read as a sentinel`,
      ).toBe('needs-allowlist');
    }
  });

  it('preserves input order and section tagging across a mixed table', () => {
    const classified = classifyEventChannelRows([
      row('§1', 'mesh-update', '`bridge.ts::onMeshUpdate`'),
      row('§1', 'claude-text-delta', '`subscribeToClaudeEvents`'),
      row('§1', 'claude-done', 'same'),
      row('§1', 'debug-request', '`gui/src` debug-bridge'),
      row('§2', 'warm-pool-event', '`bridge.ts::onWarmPoolEvent` → `WarmPoolDebugPanel`'),
    ]);

    expect(classified.map((c) => [c.section, c.channel, c.kind])).toStrictEqual([
      ['§1', 'mesh-update', 'named'],
      ['§1', 'claude-text-delta', 'named'],
      ['§1', 'claude-done', 'inherited'],
      ['§1', 'debug-request', 'needs-allowlist'],
      ['§2', 'warm-pool-event', 'named'],
    ]);
  });

  it('returns [] for no rows', () => {
    expect(classifyEventChannelRows([])).toStrictEqual([]);
  });
});

describe('unknownConsumers', () => {
  const EXPORTS = ['onMeshUpdate', 'onWarmPoolEvent', 'subscribeToClaudeEvents'];

  it('reports nothing when every doc-named consumer is a runtime export', () => {
    expect(
      unknownConsumers(EXPORTS, [
        classified('mesh-update', ['onMeshUpdate'], 'named'),
        classified('claude-text-delta', ['subscribeToClaudeEvents'], 'named'),
        classified('claude-done', ['subscribeToClaudeEvents'], 'inherited'),
        classified('debug-request', [], 'needs-allowlist'),
        classified('diagnostics', [], 'explicit-none'),
      ]),
    ).toStrictEqual([]);
  });

  it('reports the live task 6227 defect: a row naming a deleted bridge export', () => {
    // MECHANICAL REPLAY of the exact drift this guard exists to catch, not a
    // comment claiming it would. Task 6227 deletes `bridge.ts::onDiagnostics`
    // from gui/src/bridge.ts; docs/gui-event-channels.md's `diagnostics` row
    // keeps naming it. Nothing in scripts/check_event_inventory.sh notices,
    // because both of its passes key on column 1 (the channel name) only.
    const docRows = [
      classified('mesh-update', ['onMeshUpdate'], 'named'),
      classified('diagnostics', ['onDiagnostics'], 'named'),
    ];
    const exportsBefore6227 = [...EXPORTS, 'onDiagnostics'];
    const exportsAfter6227 = exportsBefore6227.filter((n) => n !== 'onDiagnostics');

    // Green before the deletion, so the finding is caused by the deletion and
    // not by the fixture being wrong.
    expect(unknownConsumers(exportsBefore6227, docRows)).toStrictEqual([]);
    expect(unknownConsumers(exportsAfter6227, docRows)).toStrictEqual([
      { channel: 'diagnostics', name: 'onDiagnostics' },
    ]);
  });

  it('checks INHERITED identifiers too, not just the cell a row spells out', () => {
    // A `same` row is only genuinely guarded if the name it inherits is checked.
    // Deleting `subscribeToClaudeEvents` must implicate all nine claude-* rows,
    // not just the one that spells the name out.
    const withoutSubscribe = EXPORTS.filter((n) => n !== 'subscribeToClaudeEvents');

    expect(
      unknownConsumers(withoutSubscribe, [
        classified('claude-text-delta', ['subscribeToClaudeEvents'], 'named'),
        classified('claude-done', ['subscribeToClaudeEvents'], 'inherited'),
      ]),
    ).toStrictEqual([
      { channel: 'claude-done', name: 'subscribeToClaudeEvents' },
      { channel: 'claude-text-delta', name: 'subscribeToClaudeEvents' },
    ]);
  });

  it('names the offending ROW, not just the symbol, and reports each row separately', () => {
    // `{channel, name}` pairs rather than bare names: a failure message has to
    // point a maintainer at the doc line to edit. One symbol named by two rows
    // is two findings.
    expect(
      unknownConsumers([], [
        classified('b-channel', ['onGone'], 'named'),
        classified('a-channel', ['onGone', 'onAlsoGone'], 'named'),
      ]),
    ).toStrictEqual([
      { channel: 'a-channel', name: 'onAlsoGone' },
      { channel: 'a-channel', name: 'onGone' },
      { channel: 'b-channel', name: 'onGone' },
    ]);
  });

  it('returns [] when there are no rows at all', () => {
    expect(unknownConsumers(EXPORTS, [])).toStrictEqual([]);
  });
});

describe('uncoveredRows', () => {
  const ALLOWLIST = {
    'debug-request': 'Consumer is the gui/src debug-bridge module, guarded by debugParity.test.ts.',
  };

  it('reports nothing when every needs-allowlist row is allowlisted', () => {
    expect(
      uncoveredRows(
        [
          classified('mesh-update', ['onMeshUpdate'], 'named'),
          classified('claude-done', ['subscribeToClaudeEvents'], 'inherited'),
          classified('diagnostics', [], 'explicit-none'),
          classified('debug-request', [], 'needs-allowlist'),
        ],
        ALLOWLIST,
      ),
    ).toStrictEqual([]);
  });

  it('reports a needs-allowlist row with no allowlist entry', () => {
    // This is the non-vacuity floor: a row the parser failed to resolve shows up
    // here and FAILS, instead of silently shrinking the checked set the way a
    // global `>= N` count floor would let it.
    expect(
      uncoveredRows(
        [
          classified('debug-request', [], 'needs-allowlist'),
          classified('mode-shape-frame', [], 'needs-allowlist', '§2'),
        ],
        ALLOWLIST,
      ),
    ).toStrictEqual(['mode-shape-frame']);
  });

  it('sorts its findings so a failure message reads as a stable list', () => {
    expect(
      uncoveredRows(
        [
          classified('zulu', [], 'needs-allowlist'),
          classified('alpha', [], 'needs-allowlist'),
          classified('mike', [], 'needs-allowlist'),
        ],
        {},
      ),
    ).toStrictEqual(['alpha', 'mike', 'zulu']);
  });

  it('does not treat an inherited Object.prototype key as an allowlist entry', () => {
    // Membership is tested with `Object.hasOwn`, never `in`. `in` walks the
    // prototype chain, and the live allowlist is a plain object literal, so a
    // channel named `constructor` would report as allowlisted against
    // `Object.prototype.constructor` with nobody having written an entry — and
    // `staleAllowlistEntries` (which iterates `Object.keys`) would not notice
    // either. `constructor` is spelled entirely within CHANNEL_ROW_RE's
    // `[a-z0-9-]+` name class, so it is a reachable channel name, not a
    // hypothetical.
    expect('constructor' in {}).toBe(true); // …which is exactly the trap.
    expect(uncoveredRows([classified('constructor', [], 'needs-allowlist')], {})).toStrictEqual([
      'constructor',
    ]);
    // An OWN entry of course still registers.
    expect(
      uncoveredRows([classified('constructor', [], 'needs-allowlist')], { constructor: 'reason' }),
    ).toStrictEqual([]);
  });

  it('returns [] for no rows', () => {
    expect(uncoveredRows([], ALLOWLIST)).toStrictEqual([]);
  });
});

describe('staleAllowlistEntries', () => {
  // Without this self-check the allowlist decays into a rubber stamp: entries
  // whose stated reason stopped being true keep suppressing checks nobody
  // re-reads. Both rot directions are covered.

  it('reports nothing when every entry names a genuinely unresolvable row', () => {
    expect(
      staleAllowlistEntries(
        [
          classified('mesh-update', ['onMeshUpdate'], 'named'),
          classified('debug-request', [], 'needs-allowlist'),
        ],
        { 'debug-request': 'prose pointer to the gui/src debug-bridge module' },
      ),
    ).toStrictEqual([]);
  });

  it('reports an entry naming a channel that no longer parses as a row', () => {
    // The channel was renamed or deleted from the doc; the excuse now protects
    // nothing.
    expect(
      staleAllowlistEntries([classified('mesh-update', ['onMeshUpdate'], 'named')], {
        'deleted-channel': 'was unwired',
      }),
    ).toStrictEqual(['deleted-channel']);
  });

  it('reports an entry whose row now names a real bridge consumer', () => {
    // `mode-shape-frame` gets wired and its Consumer cell starts naming
    // `bridge.ts::onModeShapeFrame`. The row is checkable now, so the excuse
    // must be removed rather than silently keeping the row unguarded.
    expect(
      staleAllowlistEntries(
        [classified('mode-shape-frame', ['onModeShapeFrame'], 'named', '§2')],
        { 'mode-shape-frame': 'unwired; consumer is the BucklingPanel Solid component' },
      ),
    ).toStrictEqual(['mode-shape-frame']);
  });

  it('reports an entry whose row now uses a sentinel', () => {
    // `inherited` and `explicit-none` rows are resolved by the classifier, so an
    // allowlist entry for either is dead weight.
    expect(
      staleAllowlistEntries(
        [
          classified('claude-done', ['subscribeToClaudeEvents'], 'inherited'),
          classified('diagnostics', [], 'explicit-none'),
        ],
        { 'claude-done': 'stale', diagnostics: 'stale' },
      ),
    ).toStrictEqual(['claude-done', 'diagnostics']);
  });

  it('returns [] for an empty allowlist', () => {
    expect(staleAllowlistEntries([classified('mesh-update', ['onMeshUpdate'], 'named')], {})).toStrictEqual(
      [],
    );
  });
});

describe('unregisteredConsumerlessRows', () => {
  // `*(none)*` must not be a weaker escape hatch than the allowlist beside it.
  // An explicit-none row contributes nothing to `unknownConsumers`, is excluded
  // from `uncoveredRows`, and is invisible to `staleAllowlistEntries` — so
  // without this check, writing `*(none)*` in a Consumer cell would silence the
  // guard for that row with no reason, no self-check and (docs-only edits not
  // running the gui suite) no gate failure either.

  const REGISTER = { diagnostics: 'LSP diagnostics are routed by the notification sink, not a bridge subscriber.' };

  it('reports nothing when every explicit-none row is registered', () => {
    expect(
      unregisteredConsumerlessRows(
        [
          classified('mesh-update', ['onMeshUpdate'], 'named'),
          classified('diagnostics', [], 'explicit-none'),
          classified('debug-request', [], 'needs-allowlist'),
        ],
        REGISTER,
      ),
    ).toStrictEqual([]);
  });

  it('reports an unregistered `*(none)*` row — the silencing edit this exists to catch', () => {
    expect(
      unregisteredConsumerlessRows(
        [
          classified('diagnostics', [], 'explicit-none'),
          classified('kernel-status', [], 'explicit-none'),
        ],
        REGISTER,
      ),
    ).toStrictEqual(['kernel-status']);
  });

  it('reports a row that inherited explicit-none through `same`', () => {
    // A `same` row below an `*(none)*` row resolves to explicit-none, so it is
    // held to the same accountability rather than riding its predecessor's entry.
    expect(unregisteredConsumerlessRows([classified('follower', [], 'explicit-none')], REGISTER)).toStrictEqual(
      ['follower'],
    );
  });

  it('does not treat an inherited Object.prototype key as a register entry', () => {
    expect(unregisteredConsumerlessRows([classified('constructor', [], 'explicit-none')], {})).toStrictEqual([
      'constructor',
    ]);
  });

  it('sorts its findings and returns [] for no rows', () => {
    expect(
      unregisteredConsumerlessRows(
        [
          classified('zulu', [], 'explicit-none'),
          classified('alpha', [], 'explicit-none'),
        ],
        {},
      ),
    ).toStrictEqual(['alpha', 'zulu']);
    expect(unregisteredConsumerlessRows([], REGISTER)).toStrictEqual([]);
  });
});

describe('staleConsumerlessEntries', () => {
  it('reports an entry naming a channel that no §1/§2 row carries any more', () => {
    // The channel was renamed or deleted, so the entry documents nothing.
    expect(
      staleConsumerlessEntries([classified('mesh-update', ['onMeshUpdate'], 'named')], {
        'deleted-channel': 'was consumer-less',
      }),
    ).toStrictEqual(['deleted-channel']);
  });

  it('tolerates an entry whose row is not (yet) explicit-none — pre-registration is the point', () => {
    // Deliberately one-directional, unlike `staleAllowlistEntries`. This register
    // SUPPRESSES nothing: the doc cell is what excuses the row, and the entry is
    // the reviewed co-signature. So an entry ahead of its row hides no checkable
    // row — and pre-registering is the only usage that keeps this guard decoupled
    // from another task's merge order. Task 6227 turns the `diagnostics` row's
    // Consumer cell into `*(none)*` while holding no lock on this file; demanding
    // the entry only AS that lands would make a cross-task doc edit red here.
    expect(
      staleConsumerlessEntries([classified('diagnostics', ['onDiagnostics'], 'named')], {
        diagnostics: 'pre-registered ahead of #6227',
      }),
    ).toStrictEqual([]);
  });

  it('reports nothing when every entry names a parsed row', () => {
    expect(
      staleConsumerlessEntries(
        [
          classified('diagnostics', [], 'explicit-none'),
          classified('mesh-update', ['onMeshUpdate'], 'named'),
        ],
        { diagnostics: 'reason' },
      ),
    ).toStrictEqual([]);
  });

  it('sorts its findings and returns [] for an empty register', () => {
    expect(staleConsumerlessEntries([], { zulu: 'r', alpha: 'r' })).toStrictEqual(['alpha', 'zulu']);
    expect(staleConsumerlessEntries([classified('mesh-update', ['onMeshUpdate'], 'named')], {})).toStrictEqual(
      [],
    );
  });
});

describe('landedConsumerlessChannels', () => {
  // Check (f)'s ITERATION SET: register entries whose row has actually landed as
  // `*(none)*`. Not simply `Object.keys(register)` — see `staleConsumerlessEntries`
  // one describe above for why a not-yet-landed entry has to skip.

  const REGISTER = {
    diagnostics: 'LSP diagnostics are routed by the notification sink, not a bridge subscriber.',
  };

  it('returns the intersection of register keys and explicit-none rows, sorted', () => {
    expect(
      landedConsumerlessChannels(
        [
          classified('zulu', [], 'explicit-none'),
          classified('mesh-update', ['onMeshUpdate'], 'named'),
          classified('alpha', [], 'explicit-none'),
        ],
        { zulu: 'reason', alpha: 'reason' },
      ),
    ).toStrictEqual(['alpha', 'zulu']);
  });

  it('excludes a pre-registered entry whose row is still named — the tolerance the register promises', () => {
    // The exact regression the review of task 6380 step-1 found: asserting
    // `kind === 'explicit-none'` over every register key makes a legitimately
    // pre-registered entry a red tree.
    expect(
      landedConsumerlessChannels([classified('diagnostics', ['onDiagnostics'], 'named')], REGISTER),
    ).toStrictEqual([]);
  });

  it('excludes a pre-registered entry whose row is inherited or needs-allowlist', () => {
    // The other two non-landed kinds. A `same` row below a named row resolves to
    // `inherited`; anything this parser cannot resolve is `needs-allowlist`.
    expect(
      landedConsumerlessChannels([classified('diagnostics', ['onDiagnostics'], 'inherited')], REGISTER),
    ).toStrictEqual([]);
    expect(
      landedConsumerlessChannels([classified('diagnostics', [], 'needs-allowlist')], REGISTER),
    ).toStrictEqual([]);
  });

  it('excludes a register key naming no parsed row at all', () => {
    // That rot direction is `staleConsumerlessEntries`' to report, so check (f)
    // never double-reports a renamed or deleted channel.
    expect(
      landedConsumerlessChannels([classified('mesh-update', ['onMeshUpdate'], 'named')], REGISTER),
    ).toStrictEqual([]);
    expect(staleConsumerlessEntries([classified('mesh-update', ['onMeshUpdate'], 'named')], REGISTER)).toStrictEqual(
      ['diagnostics'],
    );
  });

  it('excludes an explicit-none row that has no register entry', () => {
    // The helper is defined over REGISTER KEYS, not over rows: an unregistered
    // `*(none)*` row is `unregisteredConsumerlessRows`' gap to report.
    expect(
      landedConsumerlessChannels(
        [classified('diagnostics', [], 'explicit-none'), classified('kernel-status', [], 'explicit-none')],
        REGISTER,
      ),
    ).toStrictEqual(['diagnostics']);
  });

  it('returns [] for an empty register', () => {
    expect(landedConsumerlessChannels([classified('diagnostics', [], 'explicit-none')], {})).toStrictEqual([]);
    expect(landedConsumerlessChannels([], {})).toStrictEqual([]);
  });
});

describe('channelRegistrationsIn', () => {
  // The code→doc matcher behind coverage check (f). Its live callers only ever
  // run it over real bridge.ts text — two non-vacuity probes that assert MATCH,
  // and the loop that asserts NO-MATCH — i.e. only the loosening-safe direction.
  // The properties its docblock actually claims (the `(`/`[` prefix, the exact
  // closing-quote backreference, the escaping) are pinned here over synthetic
  // sources, so dropping one surfaces as a failure rather than as a mystery
  // false positive on some unrelated future channel.

  it('matches the direct `listen<T>(...)` registration shape', () => {
    expect(channelRegistrationsIn(`listen<MeshPayload>('mesh-update', cb);`, 'mesh-update')).toStrictEqual([
      `('mesh-update'`,
    ]);
  });

  it('matches a `[channel, mapper]` tuple entry', () => {
    // The second shape bridge.ts uses (`subscribeToClaudeEvents`); a bare
    // `listen('` matcher would miss every one of these.
    expect(
      channelRegistrationsIn(`const entries = [['claude-text-delta', toDelta]];`, 'claude-text-delta'),
    ).toStrictEqual([`['claude-text-delta'`]);
  });

  it('matches across a newline and indent between the bracket and the quote', () => {
    expect(channelRegistrationsIn(`listen(\n      'mesh-update',\n      cb,\n    );`, 'mesh-update')).toHaveLength(
      1,
    );
  });

  it('matches double-quote and backtick spellings', () => {
    expect(channelRegistrationsIn(`listen("mesh-update", cb);`, 'mesh-update')).toStrictEqual(['("mesh-update"']);
    expect(channelRegistrationsIn('listen(`mesh-update`, cb);', 'mesh-update')).toStrictEqual([
      '(`mesh-update`',
    ]);
  });

  it('returns every site, not just the first', () => {
    // (f) compares against `[]`, so one hit is enough to fail it — but the
    // docblock promises EVERY place, and a caller reporting sites wants them all.
    expect(
      channelRegistrationsIn(`listen('a', x);\nconst e = [['a', y]];\nlisten('a', z);`, 'a'),
    ).toHaveLength(3);
  });

  it('keeps prose and bare identifiers out — the bracket-prefix property', () => {
    // Without the `[([]` prefix these would both match, and every mention of a
    // channel name in a comment would read as a registration.
    expect(channelRegistrationsIn(`// the 'mesh-update' channel is emitted by main.rs`, 'mesh-update')).toStrictEqual(
      [],
    );
    expect(channelRegistrationsIn(`const meshUpdate = resolve(meshUpdate);`, 'meshUpdate')).toStrictEqual([]);
  });

  it('matches the name EXACTLY, not as a substring — the closing-backreference property', () => {
    // The bug this style of matcher usually ships with. Without the `\1`
    // backreference, asking for `diagnostics` would count the unrelated
    // `tessellation-diagnostics` channel as a registration of it.
    expect(channelRegistrationsIn(`listen('tessellation-diagnostics', cb);`, 'diagnostics')).toStrictEqual([]);
    expect(channelRegistrationsIn(`listen('diagnostics-raw', cb);`, 'diagnostics')).toStrictEqual([]);
  });

  it('ignores a quote-mismatched literal', () => {
    expect(channelRegistrationsIn(`listen("mesh-update', cb);`, 'mesh-update')).toStrictEqual([]);
  });

  it('escapes regex metacharacters in the channel name', () => {
    // An exported helper over a caller-supplied name: unescaped, `(` throws a
    // SyntaxError at construction and `.` silently wildcards. Today's only
    // caller passes parser-derived `[a-z0-9-]+` names, so nothing else pins this.
    expect(() => channelRegistrationsIn(`listen('a(b', cb);`, 'a(b')).not.toThrow();
    expect(channelRegistrationsIn(`listen('a(b', cb);`, 'a(b')).toStrictEqual([`('a(b'`]);
    expect(channelRegistrationsIn(`listen('abc', cb);`, 'a.c')).toStrictEqual([]);
    expect(channelRegistrationsIn(`listen('a.c', cb);`, 'a.c')).toStrictEqual([`('a.c'`]);
  });

  it('also matches non-subscription call sites — a documented over-match', () => {
    // `(` before the quote is any call position, so this is a hit. Deliberate:
    // a channel named in `invoke(...)` inside bridge.ts is still worth a look
    // from (f). Pinned so (f)'s failure message keeps saying "in a registration
    // position" rather than the narrower, and here wrong, "subscribes to".
    expect(channelRegistrationsIn(`invoke('diagnostics');`, 'diagnostics')).toStrictEqual([`('diagnostics'`]);
  });

  it('returns [] for an empty source', () => {
    expect(channelRegistrationsIn('', 'mesh-update')).toStrictEqual([]);
  });
});
