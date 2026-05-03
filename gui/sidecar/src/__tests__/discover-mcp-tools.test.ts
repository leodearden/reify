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
    // The TS discovery layer intentionally tolerates any casing via [A-Za-z0-9_]+.
    // The Rust convention (snake_case) is enforced by the Rust layer; keeping the
    // TS layer casing-agnostic means the test stays valid if a mixed-case name is
    // ever introduced or if the convention is relaxed.  See design decision in plan.json
    // ("tolerates any casing") and the REGISTER_LITERAL_RE comment in discover-mcp-tools.ts.
    const dir = makeTempDir();
    writeFileSync(
      join(dir, 'navigation.rs'),
      `pub fn register(registry: &mut Registry) {\n    registry.register("reify_GetSource", handler);\n}\n`,
    );
    const result = discoverRegisteredTools(dir);
    expect(result.has('reify_GetSource')).toBe(true);
  });

  it('ignores a const NAME with no matching registry.register(NAME, ...) call', () => {
    // Regression test for CONST_DECL_RE gating: a stale or test-only const must
    // NOT appear in the discovered set unless its NAME also appears as the first
    // argument to a registry.register(NAME, ...) call in the same file.
    // The real inline registration for a different tool must still be discovered.
    const dir = makeTempDir();
    writeFileSync(
      join(dir, 'stale.rs'),
      [
        '// Stale const that is never wired into the registry:',
        'const STALE: &str = "reify_stale";',
        '',
        'pub fn register(registry: &mut Registry) {',
        '    registry.register("reify_real", handler);',
        '}',
        '',
      ].join('\n'),
    );
    const result = discoverRegisteredTools(dir);
    expect(result.has('reify_stale')).toBe(false);
    expect(result.has('reify_real')).toBe(true);
  });

  it('throws an Error containing the resolved path when given a non-existent directory', () => {
    const nonExistent = resolve(tmpdir(), 'reify-tools-does-not-exist-98765');
    expect(() => discoverRegisteredTools(nonExistent)).toThrowError(nonExistent);
  });
});
