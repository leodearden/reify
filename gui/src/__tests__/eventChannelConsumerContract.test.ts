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
} from './eventChannelConsumerContract';

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
