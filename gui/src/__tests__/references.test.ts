import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { EditorView } from '@codemirror/view';
import { flushMacrotasks, withSuppressedRejections } from './test-utils';
import type { Location, LspClient } from '../editor/lspClient';

// Mock Tauri + CodeMirror modules so importing references.ts (which only needs
// the EditorView / Location / LspClient *types*) never drags real DOM or Tauri
// runtime deps into the jsdom test. Mirrors gotoDefinition.test.ts.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@codemirror/view', () => ({
  EditorView: {
    domEventHandlers: (handlers: Record<string, Function>) => ({ handlers }),
  },
}));

vi.mock('@codemirror/state', () => ({
  // Minimal mock
}));

import { findUsesCommand } from '../editor/references';

beforeEach(() => {
  vi.clearAllMocks();
});

/**
 * Factory for a minimal mock EditorView used by findUsesCommand tests.
 *
 * - `head` drives view.state.selection.main.head (the cursor offset).
 * - `lineAt(pos)` returns { number, from } so the command can derive the
 *   0-based LSP line (number - 1) and character (head - from).
 * - `line(n)` returns { text } and powers the best-effort preview lookup
 *   (view.state.doc.line(loc.range.start.line + 1).text).
 */
function makeMockView(overrides?: {
  head?: number;
  lineAt?: (pos: number) => { number: number; from: number };
  line?: (n: number) => { text: string };
}) {
  return {
    state: {
      selection: { main: { head: overrides?.head ?? 5 } },
      doc: {
        lineAt: overrides?.lineAt ?? ((_pos: number) => ({ number: 1, from: 0 })),
        line: overrides?.line ?? ((n: number) => ({ text: `text-for-line-${n}` })),
      },
    },
  } as unknown as EditorView;
}

describe('findUsesCommand', () => {
  it('(a) returns a function (CodeMirror Command)', () => {
    const references = vi.fn().mockResolvedValue([]);
    const client: Pick<LspClient, 'references'> = { references };
    const command = findUsesCommand(() => 'file:///test.ri', client, vi.fn());
    expect(typeof command).toBe('function');
  });

  it('reads the cursor, calls client.references(uri, lspLine, lspChar, true), and forwards N ReferenceResults (0-based line/character preserved)', async () => {
    const uri = 'file:///test.ri';
    // Member used 3× (declaration ∪ uses). Distinct lines so the mapping is
    // unambiguous and an "everything is 0" false positive cannot hide.
    const locations: Location[] = [
      { uri, range: { start: { line: 2, character: 4 }, end: { line: 2, character: 9 } } },
      { uri, range: { start: { line: 5, character: 8 }, end: { line: 5, character: 13 } } },
      { uri, range: { start: { line: 9, character: 0 }, end: { line: 9, character: 5 } } },
    ];
    const references = vi.fn().mockResolvedValue(locations);
    const onResults = vi.fn();
    const client: Pick<LspClient, 'references'> = { references };
    const command = findUsesCommand(() => uri, client, onResults);

    // head=15 on a line starting at offset 10 with line number 3 →
    // lspLine = 3 - 1 = 2, lspChar = 15 - 10 = 5 (non-trivial conversion).
    const view = makeMockView({
      head: 15,
      lineAt: (_pos: number) => ({ number: 3, from: 10 }),
      line: (n: number) => ({ text: `  use-on-line-${n}  ` }),
    });

    const result = command(view);
    expect(result).toBe(true); // always consumes the key

    await flushMacrotasks();

    // includeDeclaration defaults to true; position derived from the live cursor.
    expect(references).toHaveBeenCalledTimes(1);
    expect(references).toHaveBeenCalledWith(uri, 2, 5, true);

    // onResults receives one ReferenceResult per Location, preserving the raw
    // 0-based LSP line/character (the +1 conversion happens later, at the App
    // click handler — not here).
    expect(onResults).toHaveBeenCalledTimes(1);
    const results = onResults.mock.calls[0][0];
    expect(results).toHaveLength(3);
    expect(results[0]).toMatchObject({ uri, line: 2, character: 4, endLine: 2, endCharacter: 9 });
    expect(results[1]).toMatchObject({ uri, line: 5, character: 8, endLine: 5, endCharacter: 13 });
    expect(results[2]).toMatchObject({ uri, line: 9, character: 0, endLine: 9, endCharacter: 5 });
    // Best-effort preview = trimmed text of doc.line(start.line + 1).
    // results[0] is start.line=2 → doc.line(3).text = "  use-on-line-3  " → trimmed.
    expect(results[0].preview).toBe('use-on-line-3');
  });

  it('uses the current URI from the getter at call time', async () => {
    let currentUri = 'file:///first.ri';
    const references = vi.fn().mockResolvedValue([]);
    const client: Pick<LspClient, 'references'> = { references };
    const command = findUsesCommand(() => currentUri, client, vi.fn());

    command(makeMockView());
    await flushMacrotasks();
    expect(references.mock.calls[0][0]).toBe('file:///first.ri');

    currentUri = 'file:///second.ri';
    references.mockClear();
    command(makeMockView());
    await flushMacrotasks();
    expect(references.mock.calls[0][0]).toBe('file:///second.ri');
  });

  it('forwards an empty list (onResults([])) when there are no references', async () => {
    const references = vi.fn().mockResolvedValue([]);
    const onResults = vi.fn();
    const client: Pick<LspClient, 'references'> = { references };
    const command = findUsesCommand(() => 'file:///test.ri', client, onResults);

    const result = command(makeMockView());
    expect(result).toBe(true);

    await flushMacrotasks();
    expect(onResults).toHaveBeenCalledWith([]);
  });

  it('emits results even when preview extraction throws (best-effort, guarded)', async () => {
    const uri = 'file:///test.ri';
    const locations: Location[] = [
      { uri, range: { start: { line: 2, character: 4 }, end: { line: 2, character: 9 } } },
    ];
    const references = vi.fn().mockResolvedValue(locations);
    const onResults = vi.fn();
    const client: Pick<LspClient, 'references'> = { references };
    const command = findUsesCommand(() => uri, client, onResults);

    // doc.line() throws → preview must be guarded per-result; the result itself
    // is still emitted (the whole command must not blow up over a preview).
    const view = makeMockView({
      head: 5,
      lineAt: (_pos: number) => ({ number: 1, from: 0 }),
      line: (_n: number) => { throw new RangeError('line out of range'); },
    });

    const result = command(view);
    expect(result).toBe(true);

    await flushMacrotasks();
    expect(onResults).toHaveBeenCalledTimes(1);
    const results = onResults.mock.calls[0][0];
    expect(results).toHaveLength(1);
    expect(results[0]).toMatchObject({ uri, line: 2, character: 4, endLine: 2, endCharacter: 9 });
    expect(results[0].preview).toBeUndefined();
  });

  it('swallows errors from client.references (no throw; onResults not called with a non-empty list) and still returns true', async () => {
    const references = vi.fn().mockRejectedValue(new Error('references blew up'));
    const onResults = vi.fn();
    const client: Pick<LspClient, 'references'> = { references };
    const command = findUsesCommand(() => 'file:///test.ri', client, onResults);

    await withSuppressedRejections(async () => {
      const result = command(makeMockView());
      expect(result).toBe(true); // key still consumed even on error

      await flushMacrotasks();

      // Error is swallowed: onResults is either never called or called with [].
      const emittedNonEmpty = onResults.mock.calls.some(
        (call) => Array.isArray(call[0]) && call[0].length > 0,
      );
      expect(emittedNonEmpty).toBe(false);
    });
  });
});
