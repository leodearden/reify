/**
 * Unit pins for the shared ToolDef-name extraction helper (task 6065).
 *
 * Every case below runs against SYNTHETIC Rust-source string literals — no
 * on-disk fixture — mirroring how gui/test/visual/sharedModuleLoad.test.ts
 * pins each reference form from a string literal.  That keeps the regex
 * semantics pinned independently of whatever debug_server.rs happens to
 * contain today.
 *
 * COVERAGE BOUNDARY.  The real-file drift guard (`names.length ===
 * rawToolDefCount - 1` against the actual debug_server.rs) deliberately lives
 * ONLY in ./debugParity.test.ts case (b).  Re-asserting it here would recreate,
 * in a new location, exactly the duplication this task removes.  The single
 * real-file touch in this file is a path-resolution smoke check, not a drift
 * guard.
 */
import { describe, it, expect } from 'vitest';

import { extractToolDefNames, analyzeToolDefs, readDebugServerSource } from './toolDefNames';

describe('extractToolDefNames', () => {
  it('captures the quoted name field of each ToolDef literal, in source order', () => {
    const src = `
      ToolDef { name: "health", description: "liveness" },
      ToolDef { name: "engine_state", description: "engine read" },
      ToolDef { name: "mesh_stats", description: "mesh read" },
    `;
    expect(extractToolDefNames(src)).toStrictEqual(['health', 'engine_state', 'mesh_stats']);
  });

  it('spans arbitrary whitespace between `ToolDef {` and `name:` (the `\\s*` in the regex)', () => {
    // Same-line form.
    expect(extractToolDefNames('ToolDef { name: "health" }')).toStrictEqual(['health']);
    // Newline-separated form — how every literal in debug_server.rs is written.
    expect(extractToolDefNames('ToolDef {\n    name: "health",\n}')).toStrictEqual(['health']);
    // `\s*` is zero-or-more, so the fully-compacted form matches too.
    expect(extractToolDefNames('ToolDef{name:"health"}')).toStrictEqual(['health']);
  });

  it("does not capture the unquoted `name: &'static str` field of `struct ToolDef {`", () => {
    const src = `
      struct ToolDef {
          name: &'static str,
          description: &'static str,
      }

      ToolDef { name: "health", description: "liveness" },
    `;
    // The struct definition contributes to the raw \`ToolDef {\` count but can
    // never contribute a name: the regex requires a string literal and the
    // struct's field type is unquoted.  That asymmetry is what makes
    // analyzeToolDefs' `-1` an identity rather than a tuned constant.
    expect(extractToolDefNames(src)).toStrictEqual(['health']);
  });

  it('drops names carrying characters outside [a-z0-9_]', () => {
    const src = `
      ToolDef { name: "health", description: "kept" },
      ToolDef { name: "Health", description: "uppercase — dropped" },
      ToolDef { name: "load-fixture", description: "hyphen — dropped" },
      ToolDef { name: "mesh_stats", description: "kept" },
    `;
    // Dropping rather than capturing is deliberate: such a name still counts
    // toward rawToolDefCount, so the drift surfaces via the raw cross-check in
    // debugParity.test.ts case (b) instead of being masked.
    expect(extractToolDefNames(src)).toStrictEqual(['health', 'mesh_stats']);
  });

  it('returns an empty array for a source containing no ToolDef literals', () => {
    expect(extractToolDefNames('fn main() { let name = "health"; }')).toStrictEqual([]);
  });
});

describe('analyzeToolDefs', () => {
  it('reports names, raw count, expected count and an empty duplicates list for a clean source', () => {
    const src = `
      struct ToolDef {
          name: &'static str,
      }

      ToolDef { name: "health", description: "liveness" },
      ToolDef { name: "engine_state", description: "engine read" },
    `;
    expect(analyzeToolDefs(src)).toStrictEqual({
      names: ['health', 'engine_state'],
      rawToolDefCount: 3,
      expectedNameCount: 2,
      duplicates: [],
    });
  });

  it('derives expectedNameCount as rawToolDefCount - 1', () => {
    const src = `
      struct ToolDef {
          name: &'static str,
      }

      ToolDef { name: "health", description: "liveness" },
    `;
    const result = analyzeToolDefs(src);
    expect(result.rawToolDefCount).toBe(2);
    expect(result.expectedNameCount).toBe(result.rawToolDefCount - 1);
  });

  it('names a repeated tool in duplicates', () => {
    const src = `
      struct ToolDef {
          name: &'static str,
      }

      ToolDef { name: "health", description: "liveness" },
      ToolDef { name: "engine_state", description: "engine read" },
      ToolDef { name: "health", description: "planted duplicate" },
    `;
    const result = analyzeToolDefs(src);
    // Reporting the offending NAME is the point: a bare set-size comparison can
    // only say `3 !== 2`.
    expect(result.duplicates).toStrictEqual(['health']);
    // A duplicate is still a captured name, so the raw cross-check alone would
    // not catch it — the two assertions are independent.
    expect(result.names.length).toBe(result.expectedNameCount);
  });

  it('lists each duplicated name once, in first-repeat order', () => {
    const src = `
      ToolDef { name: "b", description: "d" },
      ToolDef { name: "a", description: "d" },
      ToolDef { name: "b", description: "d" },
      ToolDef { name: "a", description: "d" },
      ToolDef { name: "b", description: "d" },
    `;
    expect(analyzeToolDefs(src).duplicates).toStrictEqual(['b', 'a']);
  });
});

describe('readDebugServerSource', () => {
  it("resolves debug_server.rs from this module's own location", () => {
    // Smoke check on the path math only (the `..` segment count is the classic
    // off-by-one trap documented in gui/test/visual/paths.ts).  The count/drift
    // assertions against this source live in ./debugParity.test.ts case (b).
    const source = readDebugServerSource();
    expect(source.length).toBeGreaterThan(0);
    expect(source).toContain('ToolDef');
  });
});
