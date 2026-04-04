import { describe, it, expect } from 'vitest';
import { isSameFile, normalizePath } from '../utils/pathUtils';

describe('normalizePath', () => {
  it('strips file:// prefix from a URI', () => {
    expect(normalizePath('file:///project/src/foo.ri')).toBe('/project/src/foo.ri');
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
});
