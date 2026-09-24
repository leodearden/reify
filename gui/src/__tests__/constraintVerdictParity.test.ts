/**
 * B1 — the two-way constraint-verdict wire pin (task 6723, PRD-4 β §4.1/§5).
 *
 * WHAT MAKES THIS "TWO-WAY".  The three constraint statuses in the fixture are
 * NOT typed here as lower-case literals.  They are whatever
 * `extractVerdictTokens(readEngineSource())` reads out of the live
 * gui/src-tauri/src/engine.rs — the producer's real bytes — pushed through the
 * REAL wire boundary (`convertRawGuiState`), the REAL reducer
 * (`createEngineStore().initFromState`) and the REAL components.  Why that,
 * rather than a hardcoded `'satisfied'`: ./constraintVerdictTokens.ts's header.
 * The contract itself is canonical on `ConstraintData.status` in
 * gui/src-tauri/src/types.rs.
 *
 * WHY NO JSX / why this is `.test.ts` and not `.test.tsx`.  Solid components
 * are plain functions returning `JSX.Element`, so `render(() => Panel(props))`
 * mounts the real component without needing JSX syntax.  That keeps this
 * module's extension aligned with its sibling helper.
 */
import { describe, it, expect, beforeAll, vi } from 'vitest';
import { createRoot } from 'solid-js';
import { render, fireEvent } from '@solidjs/testing-library';

// Mock the bridge wholesale — engineStore subscribes to every channel on
// creation. Copied from ./engineStore.test.ts:21-43, the established harness.
vi.mock('../bridge', () => ({
  onMeshUpdate: vi.fn(),
  onValueUpdate: vi.fn(),
  onConstraintUpdate: vi.fn(),
  onEvaluationStatus: vi.fn(),
  onMeshRemoved: vi.fn(),
  onValueRemoved: vi.fn(),
  onConstraintRemoved: vi.fn(),
  onTessellationDiagnostics: vi.fn(),
  onCompileDiagnostics: vi.fn(),
  onKernelStatus: vi.fn(),
  onAutoResolveStart: vi.fn(),
  onAutoResolveIteration: vi.fn(),
  onAutoResolveComplete: vi.fn(),
  onSolverProgress: vi.fn(() => Promise.resolve(() => {})),
  cancelSolve: vi.fn(() => Promise.resolve()),
  onFeaDiagnosticsChanged: vi.fn(() => Promise.resolve(() => {})),
  onFeaConvergenceChanged: vi.fn(() => Promise.resolve(() => {})),
  onTensegrityWiresUpdate: vi.fn(() => Promise.resolve(() => {})),
  onTensegritySurfacesUpdate: vi.fn(() => Promise.resolve(() => {})),
  onDisplayPanesUpdate: vi.fn(() => Promise.resolve(() => {})),
  onDisplayAppearanceUpdate: vi.fn(() => Promise.resolve(() => {})),
}));

import { convertRawGuiState } from '../types';
import type { ConstraintData, RawGuiState } from '../types';
import { createEngineStore } from '../stores/engineStore';
import { ConstraintPanel } from '../panels/ConstraintPanel';
import { StatusBar } from '../panels/StatusBar';
import { ChatPanel } from '../panels/ChatPanel';
import { createClaudeStore } from '../stores/claudeStore';
import { extractVerdictTokens, readEngineSource } from './constraintVerdictTokens';

/**
 * The payload order `build_constraints` produces for the three-verdict source:
 * it sorts by `node_id` ascending, so `#constraint[0]`/`[1]`/`[2]` arrive as
 * Satisfied / Violated / Indeterminate.  That is PRD §5 B1's "✓/✗/? in that
 * order" — the PAYLOAD order, deliberately not the rendered DOM order (see the
 * STATUS_PRIORITY case at the bottom of this file).  The Rust-side counterpart
 * of this ordering is pinned in
 * gui/src-tauri/src/tests/engine_tests.rs's TRI_VERDICT_SRC test.
 */
const SATISFIED_ID = 'TriVerdict#constraint[0]';
const VIOLATED_ID = 'TriVerdict#constraint[1]';
const INDETERMINATE_ID = 'TriVerdict#constraint[2]';

/** State as the real reducer produced it, shared by every case below. */
let constraints: Record<string, ConstraintData>;

beforeAll(() => {
  // (1) The producer's real tokens, read from engine.rs. Never a literal.
  const tokens = extractVerdictTokens(readEngineSource());

  // (2) A wire-format payload carrying exactly those tokens.
  const raw: RawGuiState = {
    meshes: [],
    values: [],
    constraints: [
      {
        node_id: SATISFIED_ID,
        expression: 'width > 10mm',
        status: tokens.get('Satisfied')!,
        label: null,
        parameter_ids: [],
      },
      {
        node_id: VIOLATED_ID,
        expression: 'thickness > 2mm',
        status: tokens.get('Violated')!,
        label: null,
        parameter_ids: [],
      },
      {
        node_id: INDETERMINATE_ID,
        expression: 'tolerance > 0.1mm',
        status: tokens.get('Indeterminate')!,
        label: null,
        parameter_ids: [],
      },
    ],
    files: [],
    tessellation_diagnostics: [],
    compile_diagnostics: [],
  };

  // (3) Through the real boundary and the real reducer.
  const guiState = convertRawGuiState(raw);
  createRoot((dispose) => {
    const store = createEngineStore();
    store.initFromState(guiState);
    constraints = { ...store.state.constraints };
    dispose();
  });
});

function renderPanel(): HTMLElement {
  // `values` only feeds the contributing-parameters list of an EXPANDED row,
  // which no case here opens — passing `{}` keeps that plain.
  return render(() => ConstraintPanel({ constraints, values: {} })).container;
}

function badgeFor(container: HTMLElement, nodeId: string): HTMLElement {
  const row = container.querySelector<HTMLElement>(`[data-testid="constraint-row-${nodeId}"]`);
  if (!row) {
    const present = [...container.querySelectorAll('[data-testid^="constraint-row-"]')]
      .map((e) => e.getAttribute('data-testid'))
      .join(', ');
    throw new Error(`no constraint row for "${nodeId}" (rows present: ${present || 'none'})`);
  }
  const badge = row.querySelector<HTMLElement>('[data-status]');
  if (!badge) throw new Error(`constraint row "${nodeId}" rendered no status badge`);
  return badge;
}

describe('constraint verdict wire contract — ConstraintPanel renders the engine’s own tokens', () => {
  it('the Satisfied verdict renders ✓ and data-status="satisfied"', () => {
    const badge = badgeFor(renderPanel(), SATISFIED_ID);
    expect(badge.textContent).toBe('✓');
    expect(badge.getAttribute('data-status')).toBe('satisfied');
  });

  it('the Violated verdict renders ✗ and data-status="violated"', () => {
    const badge = badgeFor(renderPanel(), VIOLATED_ID);
    expect(badge.textContent).toBe('✗');
    expect(badge.getAttribute('data-status')).toBe('violated');
  });

  it('the Indeterminate verdict renders ? and data-status="indeterminate"', () => {
    const badge = badgeFor(renderPanel(), INDETERMINATE_ID);
    expect(badge.textContent).toBe('?');
    expect(badge.getAttribute('data-status')).toBe('indeterminate');
  });
});

describe('constraint verdict wire contract — StatusBar counts the engine’s own tokens', () => {
  it('reads 1 satisfied / 1 violated / 1 indeterminate', () => {
    const { container } = render(() =>
      StatusBar({ evalStatus: { phase: 'idle' }, meshes: {}, constraints }),
    );
    const count = (status: string): string | null =>
      container.querySelector(`[data-status="${status}"]`)?.textContent ?? null;

    // `constraintSummary` buckets anything it does not recognise as
    // indeterminate, so a casing desync shows up here as 0 / 0 / 3.
    expect(count('satisfied')).toBe('1');
    expect(count('violated')).toBe('1');
    expect(count('indeterminate')).toBe('1');
  });
});

describe('constraint verdict wire contract — the violated-first sort', () => {
  it('renders violated → indeterminate → satisfied, not payload order', () => {
    // DELIBERATE, and pinned here rather than left to be frozen by accident:
    // ConstraintPanel.tsx's STATUS_PRIORITY = { violated: 0, indeterminate: 1,
    // satisfied: 2 } re-sorts problems to the top, so the rendered order is NOT
    // the ✓/✗/? payload order the engine emits. That is also why the badge
    // cases above anchor on `data-testid="constraint-row-<node_id>"` instead of
    // asserting DOM order.
    //
    // It doubles as a desync detector: STATUS_PRIORITY is keyed lower-case and
    // falls back to `?? 1`, so under a PascalCase payload every row scores 1,
    // the sort is a no-op, and this reads back in payload order instead.
    const ids = [...renderPanel().querySelectorAll('[data-testid^="constraint-row-"]')].map((e) =>
      e.getAttribute('data-testid')?.replace('constraint-row-', ''),
    );
    expect(ids).toStrictEqual([VIOLATED_ID, INDETERMINATE_ID, SATISFIED_ID]);
  });
});


describe('constraint verdict wire contract — ChatPanel gates on the engine’s own tokens', () => {
  it('offers the "Violated constraints" context only when a violated token is present', () => {
    // `hasViolatedConstraints` is the consumer whose desync is SILENT: no wrong
    // glyph, no wrong count — the "Violated constraints" context option simply
    // never enables, so the affordance goes missing with nothing to see.
    const violatedOption = (engineConstraints: ConstraintData[]): HTMLButtonElement => {
      const store = createClaudeStore({
        onSend: vi.fn(),
        onAbort: vi.fn(),
        onPermissionDecision: vi.fn(),
      });
      const { container } = render(() => ChatPanel({ store, engineConstraints, diagnostics: [] }));
      fireEvent.click(container.querySelector<HTMLElement>('[data-testid="context-picker-btn"]')!);
      const option = [...container.querySelectorAll('button')].find(
        (b) => b.textContent === 'Violated constraints',
      );
      if (!option) throw new Error('the context picker rendered no "Violated constraints" option');
      return option;
    };

    expect(violatedOption(Object.values(constraints)).disabled).toBe(false);
    // Control: the option tracks the predicate rather than being always-enabled.
    expect(violatedOption([constraints[SATISFIED_ID]]).disabled).toBe(true);
  });
});
