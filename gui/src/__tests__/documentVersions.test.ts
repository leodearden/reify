/**
 * Unit tests for the per-document LSP version counter
 * (gui/src/editor/documentVersions.ts).
 *
 * The property under test is INDEPENDENCE: each URI advances on its own. The
 * single shared `lspVersion` counter this module replaces violated that — a
 * didChange to one file bumped the number every other file's next notification
 * carried, so a server-stamped version could never be compared against "the
 * version the client last sent for THIS document".
 */
import { describe, it, expect } from 'vitest';
import { createDocumentVersions, versionsDisagree } from '../editor/documentVersions';

const A = 'file:///proj/a.ri';
const B = 'file:///proj/b.ri';

describe('createDocumentVersions', () => {
  it('reports no version for a URI that was never opened', () => {
    const versions = createDocumentVersions();
    expect(versions.current(A)).toBeUndefined();
  });

  it('starts a document at version 1', () => {
    const versions = createDocumentVersions();
    expect(versions.next(A)).toBe(1);
    expect(versions.current(A)).toBe(1);
  });

  it('increments a document on each next() and tracks it in current()', () => {
    const versions = createDocumentVersions();
    expect(versions.next(A)).toBe(1);
    expect(versions.next(A)).toBe(2);
    expect(versions.next(A)).toBe(3);
    expect(versions.current(A)).toBe(3);
  });

  it('advances two documents INDEPENDENTLY', () => {
    const versions = createDocumentVersions();
    versions.next(A);
    versions.next(A);
    versions.next(B);

    expect(versions.current(A)).toBe(2);
    // B is at 1, NOT 3: B's counter is untouched by edits to A.
    expect(versions.current(B)).toBe(1);
  });

  it('forget() drops only that URI, and a later next() restarts it at 1', () => {
    const versions = createDocumentVersions();
    versions.next(A);
    versions.next(A);
    versions.next(B);

    versions.forget(A);

    expect(versions.current(A)).toBeUndefined();
    expect(versions.current(B)).toBe(1);
    // Reopening the file starts a fresh didOpen sequence.
    expect(versions.next(A)).toBe(1);
  });

  it('forget() on an untracked URI is a no-op', () => {
    const versions = createDocumentVersions();
    versions.next(A);

    versions.forget(B);

    expect(versions.current(A)).toBe(1);
    expect(versions.current(B)).toBeUndefined();
  });
});

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
