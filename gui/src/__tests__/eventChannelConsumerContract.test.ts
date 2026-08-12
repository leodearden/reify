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
  splitTableCells,
  parseEventChannelRows,
  extractConsumerIdentifiers,
  classifyEventChannelRows,
  unknownConsumers,
  uncoveredRows,
  staleAllowlistEntries,
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

  it('inherits from the nearest preceding named row, not the first one in the section', () => {
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

  it('degrades a `same` row with no preceding named row to needs-allowlist rather than throwing', () => {
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

  it('does not let an `*(none)*` row become the inheritance source for a later `same`', () => {
    // `*(none)*` asserts "no consumer", so a following `same` must not resolve
    // to it — there is nothing to inherit. It falls back to the last row that
    // actually named a consumer.
    const classified = classifyEventChannelRows([
      row('§1', 'a', '`onA`'),
      row('§1', 'diagnostics', '*(none)*'),
      row('§1', 'c', 'same'),
    ]);

    expect(classified[2].identifiers).toStrictEqual(['onA']);
    expect(classified[2].kind).toBe('inherited');
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
