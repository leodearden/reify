/**
 * Unit tests for the shared version-skew contract
 * (gui/src/editor/versionSkew.ts).
 */
import { describe, it, expect, vi } from 'vitest';
import type { EditorView } from '@codemirror/view';
import { createDocumentVersions } from '../editor/documentVersions';
import { askFresh, versionsDisagree, type VersionSkewGuard } from '../editor/versionSkew';

const A = 'file:///proj/a.ri';

describe('versionsDisagree', () => {
  it('reports disagreement when both versions are known and differ', () => {
    expect(versionsDisagree(1, 2)).toBe(true);
    expect(versionsDisagree(2, 1)).toBe(true);
  });

  it('reports agreement when both versions are known and equal', () => {
    expect(versionsDisagree(3, 3)).toBe(false);
  });

  it('treats an unknown side as unknowable, not as proven disagreement', () => {
    // The server stated no version.
    expect(versionsDisagree(null, 2)).toBe(false);
    // The client never captured one.
    expect(versionsDisagree(undefined, 2)).toBe(false);
    // The client no longer tracks the URI (e.g. after forget()).
    expect(versionsDisagree(2, undefined)).toBe(false);
  });

  it('sees a version the counter has moved past, and goes blind once the URI is forgotten', () => {
    const versions = createDocumentVersions();
    const asked = versions.next(A);
    versions.next(A);
    expect(versionsDisagree(asked, versions.current(A))).toBe(true);

    versions.forget(A);
    expect(versionsDisagree(asked, versions.current(A))).toBe(false);
  });
});

describe('askFresh', () => {
  const view = {} as EditorView;
  const syncedGuard = (): VersionSkewGuard => ({
    currentVersion: vi.fn(() => undefined),
    syncServer: vi.fn(() => null),
  });

  it('returns a fresh first answer without asking again', async () => {
    const ask = vi.fn().mockResolvedValue('answer');

    await expect(askFresh(syncedGuard(), view, ask, () => false)).resolves.toBe('answer');
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('returns an answer it does not judge stale as is, null included', async () => {
    const ask = vi.fn().mockResolvedValue(null);

    await expect(askFresh(syncedGuard(), view, ask, (answer) => answer !== null)).resolves.toBeNull();
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('asks again after a stale answer and returns the second', async () => {
    const ask = vi.fn().mockResolvedValueOnce('stale').mockResolvedValueOnce('fresh');

    await expect(askFresh(syncedGuard(), view, ask, (answer) => answer === 'stale')).resolves.toBe('fresh');
    expect(ask).toHaveBeenCalledTimes(2);
  });

  it('gives up with null when the second answer is stale too, never asking a third time', async () => {
    const ask = vi.fn().mockResolvedValue('stale');

    await expect(askFresh(syncedGuard(), view, ask, () => true)).resolves.toBeNull();
    expect(ask).toHaveBeenCalledTimes(2);
  });

  it('syncs the server before and after every ask, and judges an answer only once synced', async () => {
    const order: string[] = [];
    const guard: VersionSkewGuard = {
      currentVersion: vi.fn(() => undefined),
      // Settles a microtask later, so a step that did not wait would be
      // recorded ahead of its 'sync'.
      syncServer: vi.fn(() => Promise.resolve().then(() => void order.push('sync'))),
    };
    const ask = vi.fn(() => {
      order.push('ask');
      return Promise.resolve('answer');
    });
    const isStale = vi
      .fn()
      .mockImplementationOnce(() => (order.push('judge'), true))
      .mockImplementationOnce(() => (order.push('judge'), false));

    await askFresh(guard, view, ask, isStale);

    expect(order).toEqual(['sync', 'ask', 'sync', 'judge', 'sync', 'ask', 'sync', 'judge']);
    expect(guard.syncServer).toHaveBeenCalledWith(view);
  });
});
