/**
 * Unit tests for the shared version-skew contract
 * (gui/src/editor/versionSkew.ts).
 */
import { describe, it, expect } from 'vitest';
import { createDocumentVersions } from '../editor/documentVersions';
import { versionsDisagree } from '../editor/versionSkew';

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
