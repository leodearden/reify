import { describe, it, expect } from 'vitest';
import { isSameFile, normalizePath, canonicalizeKey, pathToUri, workspaceRootUriForFile } from '../utils/pathUtils';

describe('normalizePath', () => {
  it('strips file:// prefix from a URI', () => {
    expect(normalizePath('file:///project/src/foo.ri')).toBe('/project/src/foo.ri');
  });

  it('decodes percent-encoded spaces in a file:// URI', () => {
    expect(normalizePath('file:///project/src/hello%20world.ri')).toBe('/project/src/hello world.ri');
  });

  it('decodes multiple percent-encoded characters in a file:// URI', () => {
    expect(normalizePath('file:///path/%E4%BD%A0%E5%A5%BD.ri')).toBe('/path/你好.ri');
  });

  it('returns stripped path without throwing on malformed percent-encoding', () => {
    expect(normalizePath('file:///path/bad%ZZsequence.ri')).toBe('/path/bad%ZZsequence.ri');
  });

  it('passes bare paths through unchanged (no file:// prefix)', () => {
    expect(normalizePath('/project/src/foo.ri')).toBe('/project/src/foo.ri');
  });
});

describe('isSameFile', () => {
  it('identical bare paths match', () => {
    expect(isSameFile('/project/src/bracket.ri', '/project/src/bracket.ri')).toBe(true);
  });

  it('identical URIs match', () => {
    expect(isSameFile('file:///project/src/bracket.ri', 'file:///project/src/bracket.ri')).toBe(true);
  });

  it('bare path vs file://-prefixed URI match', () => {
    expect(isSameFile('/project/src/bracket.ri', 'file:///project/src/bracket.ri')).toBe(true);
  });

  it('file://-prefixed URI vs bare path match', () => {
    expect(isSameFile('file:///project/src/bracket.ri', '/project/src/bracket.ri')).toBe(true);
  });

  it('genuinely different paths do not match', () => {
    expect(isSameFile('/project/src/bracket.ri', '/project/src/mount.ri')).toBe(false);
  });

  it('partial path overlap does not false-positive', () => {
    // '/b/a/foo.ri' should NOT match '/a/foo.ri' even though it ends with '/a/foo.ri'
    expect(isSameFile('/a/foo.ri', '/b/a/foo.ri')).toBe(false);
  });

  it('empty strings match each other', () => {
    expect(isSameFile('', '')).toBe(true);
  });

  it('empty string does not match any real path', () => {
    expect(isSameFile('', '/project/src/bracket.ri')).toBe(false);
    expect(isSameFile('/project/src/bracket.ri', '')).toBe(false);
  });

  it('matches a percent-encoded URI against its decoded bare-path equivalent', () => {
    expect(isSameFile('/project/hello world.ri', 'file:///project/hello%20world.ri')).toBe(true);
  });
});

describe('canonicalizeKey', () => {
  // (a) delegates file:// stripping and percent-decoding to normalizePath
  it('strips file:// prefix and decodes percent-encoding', () => {
    expect(canonicalizeKey('file:///a/foo.ri')).toBe('/a/foo.ri');
  });
  it('decodes percent-encoded chars after file:// stripping', () => {
    expect(canonicalizeKey('file:///a/hello%20world.ri')).toBe('/a/hello world.ri');
  });

  // (b) collapses ./ segments
  it("collapses './' segments in an absolute path", () => {
    expect(canonicalizeKey('/a/./b/foo.ri')).toBe('/a/b/foo.ri');
  });
  it("collapses a leading './' in an absolute path", () => {
    expect(canonicalizeKey('/a/./foo.ri')).toBe('/a/foo.ri');
  });

  // (c) collapses .. segments
  it("resolves '..' to parent directory", () => {
    expect(canonicalizeKey('/a/b/../foo.ri')).toBe('/a/foo.ri');
  });
  it("resolves multiple '..' segments", () => {
    expect(canonicalizeKey('/a/b/c/../../foo.ri')).toBe('/a/foo.ri');
  });

  // (d) already-canonical path unchanged
  it('leaves an already-canonical absolute path unchanged', () => {
    expect(canonicalizeKey('/a/b/foo.ri')).toBe('/a/b/foo.ri');
  });

  // (e) does NOT try to resolve relative paths to absolute
  it('returns relative path unchanged (cannot CWD in pure TS)', () => {
    expect(canonicalizeKey('relative/foo.ri')).toBe('relative/foo.ri');
  });

  // (f) repeated slashes collapse
  it('collapses repeated slashes', () => {
    expect(canonicalizeKey('/a//b///foo.ri')).toBe('/a/b/foo.ri');
  });

  // (g) trailing slash on non-root path removed
  it('removes trailing slash from a non-root path', () => {
    expect(canonicalizeKey('/a/b/')).toBe('/a/b');
  });
  it('leaves the root "/" unchanged', () => {
    expect(canonicalizeKey('/')).toBe('/');
  });
});

describe('pathToUri', () => {
  it('converts an absolute path to a file:// URI', () => {
    expect(pathToUri('/a/b.ri')).toBe('file:///a/b.ri');
  });

  it('returns an already-file:// input unchanged', () => {
    expect(pathToUri('file:///a/b.ri')).toBe('file:///a/b.ri');
  });

  it('inserts a leading slash for a path without one', () => {
    // 'b.ri' → 'file:///b.ri' (mirrors Editor.tsx private closure semantics)
    expect(pathToUri('b.ri')).toBe('file:///b.ri');
  });
});

describe('workspaceRootUriForFile', () => {
  // (a) bare absolute path: returns directory as file:// URI
  it('returns the parent directory as a file:// URI for a bare absolute path', () => {
    expect(workspaceRootUriForFile('/proj/sub/main.ri')).toBe('file:///proj/sub');
  });

  // (b) nested path
  it('returns the correct parent directory for a deeply nested file', () => {
    expect(workspaceRootUriForFile('/a/b/c/file.ri')).toBe('file:///a/b/c');
  });

  // (c) already file://-prefixed input: strips scheme, derives parent, re-encodes
  it('handles an already-file://-prefixed input', () => {
    expect(workspaceRootUriForFile('file:///proj/sub/main.ri')).toBe('file:///proj/sub');
  });

  // (d) percent-encoded URI: decodes first, derives parent, returns clean URI
  it('handles a percent-encoded file:// URI', () => {
    expect(workspaceRootUriForFile('file:///proj/hello%20world/main.ri')).toBe('file:///proj/hello%20world');
  });

  // (e) null → undefined (single-file fallback preserved)
  it('returns undefined for null (single-file fallback)', () => {
    expect(workspaceRootUriForFile(null)).toBeUndefined();
  });

  // (f) empty string → undefined
  it('returns undefined for an empty string', () => {
    expect(workspaceRootUriForFile('')).toBeUndefined();
  });

  // (g) file directly in root: parent is '/'
  it('returns the root URI for a file directly under root', () => {
    expect(workspaceRootUriForFile('/main.ri')).toBe('file:///');
  });

  // (h) consistent with canonicalizeKey: ..-segments are resolved before parent extraction
  it('resolves dot-dot segments before extracting the parent', () => {
    expect(workspaceRootUriForFile('/a/b/../main.ri')).toBe('file:///a');
  });
});
