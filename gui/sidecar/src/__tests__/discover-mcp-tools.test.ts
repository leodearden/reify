import { describe, it, expect, afterEach } from 'vitest';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { discoverRegisteredTools } from './discover-mcp-tools.js';

describe('discoverRegisteredTools', () => {
  const tempDirs: string[] = [];

  function makeTempDir(): string {
    const dir = mkdtempSync(join(tmpdir(), 'reify-tools-'));
    tempDirs.push(dir);
    return dir;
  }

  afterEach(() => {
    for (const dir of tempDirs.splice(0)) {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('discovers a tool registered with a literal lowercase name', () => {
    const dir = makeTempDir();
    writeFileSync(
      join(dir, 'read.rs'),
      `pub fn register(registry: &mut Registry) {\n    registry.register("reify_get_source", handler);\n}\n`,
    );
    const result = discoverRegisteredTools(dir);
    expect(result).toBeInstanceOf(Set);
    expect(result.has('reify_get_source')).toBe(true);
  });

  it('discovers a tool whose registration uses const NAME indirection', () => {
    const dir = makeTempDir();
    writeFileSync(
      join(dir, 'write.rs'),
      `const NAME: &str = "reify_qux";\n\npub fn register(registry: &mut Registry) {\n    registry.register(NAME, handler);\n}\n`,
    );
    const result = discoverRegisteredTools(dir);
    expect(result.has('reify_qux')).toBe(true);
  });

  it('discovers a tool whose name has uppercase characters', () => {
    const dir = makeTempDir();
    writeFileSync(
      join(dir, 'navigation.rs'),
      `pub fn register(registry: &mut Registry) {\n    registry.register("reify_GetSource", handler);\n}\n`,
    );
    const result = discoverRegisteredTools(dir);
    expect(result.has('reify_GetSource')).toBe(true);
  });

  it('throws an Error containing the resolved path when given a non-existent directory', () => {
    const nonExistent = resolve(tmpdir(), 'reify-tools-does-not-exist-98765');
    expect(() => discoverRegisteredTools(nonExistent)).toThrowError(nonExistent);
  });
});
