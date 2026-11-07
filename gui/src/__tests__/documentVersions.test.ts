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
import { createDocumentVersions } from '../editor/documentVersions';

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
