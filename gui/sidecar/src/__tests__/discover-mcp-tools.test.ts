import { describe, it, expect, afterEach } from 'vitest';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
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

  // NOTE: the `registry.register(NAME, handler)` line in this fixture is now
  // load-bearing: removing it would cause the gating logic (REGISTER_IDENT_RE
  // pre-pass) to exclude "reify_qux".  See the "ignores a const NAME with no
  // matching registry.register(NAME, ...) call" test below for the regression.
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

  // Regression test: comment stripping in the REGISTER_IDENT_RE pre-pass prevents
  // commented-out calls from re-admitting stale consts (see discover-mcp-tools.ts).
  it('ignores a stale const when its only registry.register call is commented out', () => {
    const dir = makeTempDir();
    writeFileSync(
      join(dir, 'commented.rs'),
      [
        '// Stale const that is wired only via a commented-out register call:',
        'const STALE: &str = "reify_stale_commented";',
        '',
        'pub fn register(registry: &mut Registry) {',
        '    // registry.register(STALE, handler);   <-- commented out!',
        '    registry.register("reify_real_in_same_file", handler);',
        '}',
        '',
      ].join('\n'),
    );
    const result = discoverRegisteredTools(dir);
    // Comment stripping removes the `// registry.register(STALE, ...)` line before
    // REGISTER_IDENT_RE runs — STALE stays out of registeredIdents and is excluded.
    expect(result.has('reify_stale_commented')).toBe(false);
    // The real inline literal registration in the same file is unaffected.
    expect(result.has('reify_real_in_same_file')).toBe(true);
  });

  // Pins the per-file gating constraint — see discover-mcp-tools.ts for the canonical
  // explanation and the future-hardening option (project-wide pre-pass).
  it('silently drops a const split across files (known per-file constraint)', () => {
    const dir = makeTempDir();
    // The const declaration lives in consts.rs …
    writeFileSync(
      join(dir, 'consts.rs'),
      `const SHARED: &str = "reify_cross_file";\n`,
    );
    // … but the registry.register(SHARED, …) call lives in a sibling file.
    writeFileSync(
      join(dir, 'register.rs'),
      [
        'pub fn register(registry: &mut Registry) {',
        '    registry.register(SHARED, handler);',
        '}',
        '',
      ].join('\n'),
    );
    const result = discoverRegisteredTools(dir);
    // The per-file gating drops the const: consts.rs has no registry.register call,
    // and register.rs has no CONST_DECL_RE match — so reify_cross_file is never added.
    expect(result.has('reify_cross_file')).toBe(false);
  });

  it('throws an Error containing the resolved path when given a non-existent directory', () => {
    const nonExistent = resolve(tmpdir(), 'reify-tools-does-not-exist-98765');
    expect(() => discoverRegisteredTools(nonExistent)).toThrowError(nonExistent);
  });

  it('discovers a tool registered in a `.rs` file inside a nested subdirectory', () => {
    const dir = makeTempDir();
    mkdirSync(join(dir, 'nested'), { recursive: true });
    writeFileSync(
      join(dir, 'nested', 'foo.rs'),
      `pub fn register(registry: &mut Registry) {\n    registry.register("reify_nested_tool", handler);\n}\n`,
    );
    const result = discoverRegisteredTools(dir);
    expect(result.has('reify_nested_tool')).toBe(true);
  });

  it('discovers tools two or more levels deep and ignores non-.rs files', () => {
    const dir = makeTempDir();
    mkdirSync(join(dir, 'nested', 'inner'), { recursive: true });
    // Two levels deep: nested/inner/bar.rs must be found.
    writeFileSync(
      join(dir, 'nested', 'inner', 'bar.rs'),
      `pub fn register(registry: &mut Registry) {\n    registry.register("reify_deep_tool", handler);\n}\n`,
    );
    // A non-.rs sibling containing a registry.register(…) substring must NOT be parsed.
    writeFileSync(
      join(dir, 'nested', 'inner', 'notes.md'),
      `registry.register("reify_should_not_be_found", handler);\n`,
    );
    const result = discoverRegisteredTools(dir);
    expect(result.has('reify_deep_tool')).toBe(true);
    expect(result.has('reify_should_not_be_found')).toBe(false);
  });
});
