/**
 * Structural coverage guard (task 6236):
 * the Consumer column of docs/gui-event-channels.md §1/§2 ↔ the runtime exports
 * of `gui/src/bridge.ts`.
 *
 * ── Invariant ────────────────────────────────────────────────────────────────
 * Every bridge-shaped identifier named in the Consumer column of a §1 or §2
 * channel row is a runtime export of `gui/src/bridge.ts`, OR its row carries a
 * documented entry in `NON_BRIDGE_CONSUMERS` below.
 *
 * ── Why a gap here is silent rather than merely wrong ────────────────────────
 * `scripts/check_event_inventory.sh` is the only other reader of this document,
 * and BOTH of its passes key exclusively on COLUMN 1 — the backticked kebab-case
 * channel name. The forward pass matches `.emit("name")` sites in tracked Rust
 * against the inventory; the `--bidirectional` reverse pass matches inventory
 * names back to quoted literals in tracked `*.rs`. Neither ever reads column 4.
 * So deleting a bridge.ts export while its channel row keeps naming it leaves
 * that lint fully green: the channel still exists, still emits, still appears.
 * The doc simply starts telling the next reader to call a function that is gone.
 *
 * Nothing else covers it either — the TS compiler never reads markdown, and
 * `bridgeMockCoverage.test.ts` compares bridge.ts against vitest mock factories,
 * not against docs.
 *
 * The live instance this guard exists for is task 6227, which deletes
 * `bridge.ts::onDiagnostics`; the `diagnostics` row names it. The detector is
 * pinned MECHANICALLY against that exact drift by
 * `eventChannelConsumerContract.test.ts`, not merely claimed here.
 *
 * ── Standing rule for docs/gui-event-channels.md ─────────────────────────────
 * A §1/§2 row's Consumer cell either names a real bridge.ts export, or writes
 * `same` (inherit the row above), or writes `*(none)*` (deliberately no
 * consumer), or its channel gets an entry in `NON_BRIDGE_CONSUMERS` with the
 * reason. Check (c) makes that exhaustive: a row matching none of the four
 * fails, so a parse miss cannot quietly shrink the guarded set.
 *
 * Shape mirrors `bridgeMockCoverage.test.ts` and `debugParity.test.ts` — the
 * house pattern for "two artifacts must stay in lockstep, with documented
 * legitimate asymmetries": mock the runtime deps, read one side authoritatively,
 * parse the other, keep a named allowlist carrying prose reasons, and add both
 * an extraction-sanity check and an allowlist-self-check so neither side can rot
 * into a rubber stamp.
 */
import { describe, it, expect, vi } from 'vitest';

// These three mocks make bridge.ts importable at collection time without a
// Tauri runtime. Reused verbatim from bridgeMockCoverage.test.ts, where the same
// set is proven sufficient for `import * as bridge from '../bridge'`.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
  open: vi.fn(),
  ask: vi.fn(),
}));

// REAL module (the three mocks above only stub its Tauri dependencies).
import * as bridge from '../bridge';

import {
  CHANNEL_ROW_SCAN_RE,
  readEventChannelInventory,
  parseEventChannelRows,
  classifyEventChannelRows,
  unknownConsumers,
  uncoveredRows,
  staleAllowlistEntries,
} from './eventChannelConsumerContract';

/**
 * Authoritative export list. `Object.keys` on the real ESM namespace needs no
 * regex and cannot drift: TS types erase, so this is exactly the set of runtime
 * exports the doc's Consumer column has to name. Deliberately NOT a `^export `
 * regex over bridge.ts — bridge.ts also has three type-only exports (one
 * `export interface`, two `export type`) that erase at runtime, and a regex
 * would accept a doc row naming one of those as if it were a callable consumer.
 */
const RUNTIME_EXPORTS = Object.keys(bridge);

const INVENTORY = readEventChannelInventory();
const ROWS = parseEventChannelRows(INVENTORY);
const CLASSIFIED = classifyEventChannelRows(ROWS);

/**
 * Channels whose Consumer cell legitimately names something that is NOT a
 * bridge.ts export, each with the reason. Check (d) keeps every entry honest in
 * the two directions it can: naming a channel no §1/§2 row carries any more, or
 * naming a row that in fact resolves on its own.
 *
 * Only TWO entries, and that is the point. The extraction rule rejects
 * non-bridge tokens MECHANICALLY — dotted store paths
 * (`engineStore.setFeaDiagnostics`), slashed module paths (`gui/src`) and the
 * PascalCase Solid component names the column legitimately carries
 * (`AutoResolvePanel`, `WarmPoolDebugPanel`, `SolverProgressOverlay`,
 * `FeaCasePickerDropdown`) — and the `same` sentinel is resolved by inheritance
 * rather than excused. Without both, this table would carry ~15 entries that all
 * said "not a bridge symbol", and an allowlist whose entries carry no
 * distinguishing reason has no signal left to give (the failure mode
 * bridgeMockCoverage.test.ts documents when it refuses to glob its target list).
 *
 * The bar for adding an entry is "this row's consumer genuinely is not a
 * bridge.ts export", NOT "the guard is complaining". A row naming a DELETED
 * bridge export must be fixed in the doc, not allowlisted here — allowlisting it
 * is exactly the drift this file exists to catch.
 */
const NON_BRIDGE_CONSUMERS: Record<string, string> = {
  'debug-request':
    'Consumer is a prose pointer to the gui/src debug-bridge module, not a bridge.ts symbol. That module has its own parity guard — debugParity.test.ts.',
  'mode-shape-frame':
    'Unwired §2 channel; its consumer is the BucklingPanel Solid component animator, owned by docs/prds/v0_5/buckling-eigensolver.md §13 task ι (#3458). Becomes checkable when the channel is wired to a bridge.ts subscriber.',
};

describe('event-channel Consumer column ↔ bridge.ts runtime exports', () => {
  it('(a) extraction sanity — neither side is vacuously empty or over-captured', () => {
    // Without this, a parser that silently returned [] would make (b) pass.
    expect(RUNTIME_EXPORTS.length).toBeGreaterThanOrEqual(60);

    // Independent recount of the same row set, by a DIFFERENT mechanism: slice
    // the document between the §1 and §2a headings by string index, then apply
    // the published grep contract line by line — no section state machine. An
    // EQUALITY rather than a `>=` floor, so a row `parseEventChannelRows` drops
    // surfaces as a mismatch instead of being masked (debugParity.test.ts's
    // `expectedNameCount` reasoning).
    const start = INVENTORY.indexOf('## §1 ');
    const end = INVENTORY.indexOf('### §2a');
    expect(start, 'the §1 heading must be findable — is the doc path right?').toBeGreaterThanOrEqual(0);
    expect(end, 'the §2a heading must be findable — is the doc path right?').toBeGreaterThan(start);
    const rawRowCount = INVENTORY.slice(start, end)
      .split('\n')
      .filter((line) => CHANNEL_ROW_SCAN_RE.test(line)).length;
    expect(ROWS.length).toBe(rawRowCount);
    expect(ROWS.length).toBeGreaterThanOrEqual(30);

    // A doubled channel is a merge artefact, not a second row to guard.
    const channels = ROWS.map((r) => r.channel);
    expect(new Set(channels).size, 'duplicate channel rows').toBe(channels.length);

    // Named regressions. Each pins one parsing hazard by name so a future
    // simplification of the extractor cannot quietly reintroduce it.
    const identifiersFor = (channel: string) =>
      CLASSIFIED.find((c) => c.channel === channel)?.identifiers;
    // Plain `bridge.ts::` cell.
    expect(identifiersFor('mesh-update')).toStrictEqual(['onMeshUpdate']);
    // Multi-token routing cell: `bridge.ts::X` → `engineStore.setY` → components.
    expect(identifiersFor('fea-diagnostics-changed')).toStrictEqual(['onFeaDiagnosticsChanged']);
    // The escaped-pipe row: its Payload cell carries `\|`, which a naive
    // `split('|')` / `awk -F'|'` would let shift the Producer cell into the
    // Consumer position, silently dropping this name from coverage.
    expect(identifiersFor('warm-pool-event')).toStrictEqual(['onWarmPoolEvent']);

    // Over-capture check: no extracted identifier may be a path, a dotted store
    // reference, or a PascalCase component name.
    for (const { channel, identifiers } of CLASSIFIED) {
      for (const name of identifiers) {
        expect(name, `${channel} extracted a non-identifier: ${name}`).toMatch(/^[A-Za-z_$][\w$]*$/);
        expect(name, `${channel} extracted a component name: ${name}`).toMatch(/^[a-z]/);
      }
    }
  });

  it('(b) every consumer the doc names is a runtime export of bridge.ts', () => {
    // THE GUARD. A failure names the doc row to edit and the symbol that is gone.
    expect(unknownConsumers(RUNTIME_EXPORTS, CLASSIFIED)).toStrictEqual([]);
  });

  it('(c) every §1/§2 row is actually guarded — none silently dropped out', () => {
    // Row-granular non-vacuity. A global `>= N` floor would let a parser that
    // dropped 5 of 40 rows still pass, and the dropped rows are precisely the
    // ones that stopped being checked. Here every row must resolve as named,
    // inherited, explicitly consumer-less, or allowlisted.
    expect(uncoveredRows(CLASSIFIED, NON_BRIDGE_CONSUMERS)).toStrictEqual([]);
  });

  it('(d) the allowlist is self-checking — no entry has rotted', () => {
    expect(staleAllowlistEntries(CLASSIFIED, NON_BRIDGE_CONSUMERS)).toStrictEqual([]);
  });
});
