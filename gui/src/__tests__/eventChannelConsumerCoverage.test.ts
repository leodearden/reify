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
 * The live instance this guard exists for was task 6227, which deleted
 * `bridge.ts::onDiagnostics`; the `diagnostics` row named it. The detector is
 * pinned MECHANICALLY against that exact drift by
 * `eventChannelConsumerContract.test.ts`, not merely claimed here.
 *
 * ── Standing rule for docs/gui-event-channels.md ─────────────────────────────
 * A §1/§2 row's Consumer cell either names a real bridge.ts export, or writes
 * `same` (inherit the row directly above), or writes `*(none)*` (deliberately no
 * consumer, and then its channel needs an entry in `DELIBERATELY_CONSUMERLESS`),
 * or its channel gets an entry in `NON_BRIDGE_CONSUMERS` with the reason. Check
 * (c) makes that exhaustive — a row matching none of the four fails, so a parse
 * miss cannot quietly shrink the guarded set — and check (e) keeps the
 * `*(none)*` bucket from being self-certifying, since a docs-only edit does not
 * run this suite.
 *
 * Shape mirrors `bridgeMockCoverage.test.ts` and `debugParity.test.ts` — the
 * house pattern for "two artifacts must stay in lockstep, with documented
 * legitimate asymmetries": mock the runtime deps, read one side authoritatively,
 * parse the other, keep a named allowlist carrying prose reasons, and add both
 * an extraction-sanity check and an allowlist-self-check so neither side can rot
 * into a rubber stamp.
 */
import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

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
  tableDataRows,
  eventChannelTableHeaders,
  parseEventChannelRows,
  classifyEventChannelRows,
  unknownConsumers,
  uncoveredRows,
  staleAllowlistEntries,
  unregisteredConsumerlessRows,
  staleConsumerlessEntries,
  landedConsumerlessChannels,
  channelRegistrationsIn,
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

/**
 * Channels whose Consumer cell is `*(none)*` — documented as deliberately having
 * no consumer — each with the reason. Check (e) requires an entry for every such
 * row, so `*(none)*` is not a weaker escape hatch than `NON_BRIDGE_CONSUMERS`
 * above: an `explicit-none` row contributes nothing to check (b), is excluded
 * from check (c), and is invisible to check (d), so without this a maintainer
 * could silence this guard for a row by editing markdown alone — with no reason,
 * no self-check, and (docs-only edits do not run the gui suite) no gate failure.
 *
 * This is a PRE-COMMITMENT list, not a post-hoc excuse list, and the direction
 * matters. `staleConsumerlessEntries` deliberately does not flag an entry whose
 * row is not yet `*(none)*`, because the entry suppresses nothing on its own —
 * the doc cell does — and because pre-registering is what keeps this guard
 * decoupled from another task's merge order. Task 6227 deleted
 * `bridge.ts::onDiagnostics` and rewrote that row's Consumer cell; it held no
 * lock on this file, so requiring the entry only as it landed would have
 * turned a cross-task doc edit into a red tree here. The reason is written
 * once, now.
 */
const DELIBERATELY_CONSUMERLESS: Record<string, string> = {
  diagnostics:
    'Task 6227 deleted bridge.ts::onDiagnostics and set this row to *(none)*: LSP diagnostics are routed by main.rs::TauriNotificationSink, with no bridge.ts subscriber left. Pre-registered so 6227 could land in either merge order without editing this file. Landed by 6227; the row now reads `*(none)*` and is accounted for here by check (e) rather than by (b).',
};

/**
 * bridge.ts source, read from disk so check (f) can assert the absence of a
 * registration SITE for each deliberately-consumer-less channel, rather than
 * the absence of one export name. Precedent for reading a target's source in a
 * guard: `bridgeMockCoverage.test.ts`.
 */
const BRIDGE_SOURCE = readFileSync(join(__dirname, '..', 'bridge.ts'), 'utf-8');

describe('event-channel Consumer column ↔ bridge.ts runtime exports', () => {
  it('(a) extraction sanity — neither side is vacuously empty or over-captured', () => {
    // Without this, a parser that silently returned [] would make (b) pass.
    expect(RUNTIME_EXPORTS.length).toBeGreaterThanOrEqual(60);

    // The Consumer column's POSITION, asserted rather than assumed. The parser
    // reads a fixed cell index; if a column were inserted ahead of Consumer it
    // would start reading Producer instead. Today that would degrade to
    // needs-allowlist by luck of the current producer spellings, not by design —
    // a producer written `emitStatus` is bridge-shaped and would pass. The parser
    // fails closed on a bad header; this names the header that went bad.
    const headers = eventChannelTableHeaders(INVENTORY);
    expect(headers.map((h) => h.section), 'one table each in §1 and §2').toStrictEqual(['§1', '§2']);
    for (const { section, cells } of headers) {
      expect(cells[4], `${section} column 4 is "${cells[4]}", not Consumer`).toBe('Consumer');
    }

    // Independent recount of the same row set, by a DIFFERENT mechanism: slice
    // the document between the §1 and §2a headings by string index, then count
    // rows line by line with no section state machine. An EQUALITY rather than a
    // `>=` floor, so a row `parseEventChannelRows` drops surfaces as a mismatch
    // instead of being masked (debugParity.test.ts's `expectedNameCount`
    // reasoning).
    const start = INVENTORY.indexOf('## §1 ');
    const end = INVENTORY.indexOf('### §2a');
    expect(start, 'the §1 heading must be findable — is the doc path right?').toBeGreaterThanOrEqual(0);
    expect(end, 'the §2a heading must be findable — is the doc path right?').toBeGreaterThan(start);
    const slice = INVENTORY.slice(start, end);

    // (i) STRUCTURAL recount — every table data row in the slice, found by
    // markdown shape alone (a `|` line that is neither a `|---|` separator nor
    // the header above one). This one shares no name class with the parser,
    // which is what makes it independent in the direction that matters: a row
    // added as `mode_shape_frame` (snake_case, as §3 already uses) or with any
    // uppercase letter is invisible to `[a-z0-9-]+` on BOTH sides, so a recount
    // built on that class would agree with a parser that silently dropped it.
    expect(ROWS.length, 'a §1/§2 table row the channel-name class does not recognise').toBe(
      tableDataRows(slice).length,
    );

    // (ii) GREP-CONTRACT recount — the same rows via the doc's own published
    // pattern, which is also what scripts/check_event_inventory.sh scans with.
    // Equal to (i) only if every §1/§2 row honours that contract, so this pins
    // the doc invariant the other lint depends on, not just this guard's parse.
    const grepContractRowCount = slice.split('\n').filter((line) => CHANNEL_ROW_SCAN_RE.test(line)).length;
    expect(ROWS.length, 'a §1/§2 row outside the published backtick grep contract').toBe(
      grepContractRowCount,
    );

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

  it('(e) every `*(none)*` row is a pre-registered, reasoned absence', () => {
    // Closes the one bucket in (c) that would otherwise be self-certifying: a
    // row can declare itself consumer-less in markdown, but only if this file
    // already says why. A finding here means someone wrote `*(none)*` without a
    // register entry — which is the doc-only silencing edit (c) cannot see.
    expect(unregisteredConsumerlessRows(CLASSIFIED, DELIBERATELY_CONSUMERLESS)).toStrictEqual([]);
    // And an entry naming a channel no §1/§2 row carries any more documents
    // nothing. See the constant's docblock for why the other rot direction is
    // deliberately tolerated.
    expect(staleConsumerlessEntries(CLASSIFIED, DELIBERATELY_CONSUMERLESS)).toStrictEqual([]);
  });

  /**
   * (f) is the code↔doc pin for EVERY registered consumer-less channel, not
   * just `diagnostics`. It started life (task 6227) as a bespoke assertion
   * hardcoded to `diagnostics` in bridge.test.ts; task 6380 generalized it here
   * and deleted that bespoke block, so a second `DELIBERATELY_CONSUMERLESS`
   * entry is protected automatically instead of needing its own hand-written
   * pin one file away from the register that explains why it exists.
   *
   * NOT redundant with check (b) (`unknownConsumers`). That check enforces only
   * doc→code: a consumer NAMED in the Consumer column must be a real runtime
   * export of bridge.ts. Nothing above enforces code→doc. An `explicit-none`
   * row asserts an ABSENCE and `DELIBERATELY_CONSUMERLESS` records why — but
   * bridge.ts re-growing a subscriber for that channel would silently falsify
   * both while every check above stayed green: an EXTRA export or an EXTRA
   * `listen(...)` call is never compared against the doc, `uncoveredRows` /
   * `staleConsumerlessEntries` read only rows, and `bridgeMockCoverage`'s
   * `notExports` looks the other way (factory key → export).
   *
   * The row itself must stay in the inventory even though it is consumer-less:
   * `scripts/check_event_inventory.sh` keys on column 1 only, so a channel that
   * still emits (as `diagnostics` does, from `main.rs::TauriNotificationSink`)
   * would report as an orphan channel if its row were deleted. Only the
   * Consumer cell moves to `*(none)*`.
   *
   * The code side asserts the CHANNEL, not an export name: a subscriber
   * re-added under any name, or folded into an already-exported helper, would
   * satisfy an `Object.keys` check while still contradicting `*(none)*`.
   * `channelRegistrationsIn` matches both registration shapes bridge.ts uses —
   * a direct `listen<T>('<channel>', ...)` call and the `['<channel>', mapper]`
   * tuple entries `subscribeToClaudeEvents` feeds to its `listen(name, mapper)`
   * loop (bridge.ts:457) — so a bare `listen('` match could not miss the second.
   *
   * DIVISION OF LABOUR with (e), which is why this loop asserts nothing about
   * rows: (e) owns register↔row (every `*(none)*` row has an entry, every entry
   * names a live row), (f) owns row↔bridge.ts. So (f) iterates
   * `landedConsumerlessChannels` rather than the raw register, and SKIPS an
   * entry whose row has not yet flipped to `*(none)*`. Such an entry pins
   * nothing — the doc cell is what asserts the absence — and demanding it have
   * landed would re-couple this suite to another task's merge order, the exact
   * asymmetry `staleConsumerlessEntries`' one-directional rule and this file's
   * `DELIBERATELY_CONSUMERLESS` docblock both exist to preserve. The floor
   * below keeps that skip from emptying the loop unnoticed.
   */
  it('(f) every deliberately-consumer-less channel has no bridge.ts registration site', () => {
    // Non-vacuity for the source matcher, one live channel per registration
    // shape: a mis-typed pattern that matched nothing would make every
    // assertion below vacuously green rather than actually checking anything.
    expect(
      channelRegistrationsIn(BRIDGE_SOURCE, 'file-changed'),
      'channel matcher found no direct `listen<T>(...)` registration in bridge.ts',
    ).not.toStrictEqual([]);
    expect(
      channelRegistrationsIn(BRIDGE_SOURCE, 'claude-text-delta'),
      'channel matcher found no tuple-entry registration in bridge.ts',
    ).not.toStrictEqual([]);

    // Register-side non-vacuity. Skipping not-yet-landed entries is correct
    // (see the docblock), but a register that drifted ENTIRELY into
    // pre-registered state would leave this loop checking nothing while staying
    // green. A `>= 1` floor, deliberately not `=== Object.keys(...).length`:
    // equality would forbid pre-registration, reintroducing in a new spelling
    // the merge-order coupling this check was fixed to shed.
    const landed = landedConsumerlessChannels(CLASSIFIED, DELIBERATELY_CONSUMERLESS);
    expect(
      landed.length,
      'no DELIBERATELY_CONSUMERLESS entry has a row that landed as `*(none)*` — (f) would check nothing',
    ).toBeGreaterThanOrEqual(1);

    for (const channel of landed) {
      expect(
        channelRegistrationsIn(BRIDGE_SOURCE, channel),
        `bridge.ts must not subscribe to the '${channel}' channel while its doc row says \`*(none)*\``,
      ).toStrictEqual([]);
    }
  });
});
