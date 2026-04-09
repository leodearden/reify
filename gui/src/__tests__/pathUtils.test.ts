import { describe, it, expect } from 'vitest';
import { isSameFile, normalizePath } from '../utils/pathUtils';

describe('normalizePath', () => {
  it('strips file:// prefix from a URI', () => {
    expect(normalizePath('file:///project/src/foo.ri')).toBe('/project/src/foo.ri');
  });

  it('decodes percent-encoded spaces in file:// URI pathname', () => {
    // Documents the downstream contract at Editor.tsx:81-83 where the result is passed
    // to bridgeOpenFile → Tauri open_file → Rust std::fs::read_to_string, which expects
    // a decoded OS path (spaces, non-ASCII). %20 must become a space.
    expect(normalizePath('file:///project/src/hello%20world.ri')).toBe('/project/src/hello world.ri');
  });

  it('decodes non-ASCII percent-encoding in file:// URI pathname', () => {
    expect(normalizePath('file:///path/%E4%BD%A0%E5%A5%BD.ri')).toBe('/path/你好.ri');
  });

  it('returns stripped path without throwing on malformed percent-encoding', () => {
    expect(normalizePath('file:///path/bad%ZZsequence.ri')).toBe('/path/bad%ZZsequence.ri');
  });

  it('passes bare paths through unchanged (no file:// prefix)', () => {
    expect(normalizePath('/project/src/foo.ri')).toBe('/project/src/foo.ri');
  });

  it('preserves %2F encoding in file:// URI pathname', () => {
    expect(normalizePath('file:///path/foo%2Fbar.ri')).toBe('/path/foo%2Fbar.ri');
  });

  it('does not decode percent-encoding in bare paths', () => {
    expect(normalizePath('/project/hello%20world.ri')).toBe('/project/hello%20world.ri');
  });

  it('does not decode %00 (null byte) in file:// URI pathname', () => {
    expect(normalizePath('file:///path/%00evil.ri')).toBe('/path/%00evil.ri');
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

  // Documents the Editor.tsx:257 contract where isSameFile compares location.file_path
  // (a file:// URI from the backend) against activeFile (a decoded bare OS path).
  it('matches a percent-encoded URI against its decoded bare-path equivalent', () => {
    expect(isSameFile('/project/hello world.ri', 'file:///project/hello%20world.ri')).toBe(true);
  });

  it('matches percent-encoded URI (first arg) against decoded bare path (second arg)', () => {
    expect(isSameFile('file:///project/hello%20world.ri', '/project/hello world.ri')).toBe(true);
  });
});
