import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parser } from '../editor/reifyParser.js';

/**
 * Corpus test for the GUI Lezer grammar (`gui/src/editor/reify.grammar`).
 *
 * The grammar is a hand-maintained subset of the authoritative
 * `tree-sitter-reify/grammar.js`. When the language surface moves ahead of
 * the Lezer port, the editor silently degrades (error nodes, lost
 * highlighting) with nothing in CI to notice. This file parses REAL committed
 * `.ri` fixtures — not reduced-subset snippets — and asserts they produce zero
 * error nodes.
 */

/** Repo root, three levels up from `gui/src/__tests__/`. */
const REPO_ROOT = join(__dirname, '../../..');

function readFixture(relPath: string): string {
  return readFileSync(join(REPO_ROOT, relPath), 'utf-8');
}

/** Parse `src` and count the error nodes the Lezer parser inserted. */
export function countErrorNodes(src: string): number {
  const tree = parser.parse(src);
  const cursor = tree.cursor();
  let errors = 0;
  do {
    if (cursor.type.isError) errors++;
  } while (cursor.next());
  return errors;
}

/**
 * SLICE A — committed fixtures greened by the structure-header delta
 * (`pub`? `structure` `def`? Name Block).
 */
const SLICE_A = [
  'examples/bracket.ri',
  'examples/m5_geometry.ri',
  'examples/sweep_degenerate.ri',
  'examples/complex_transcendental.ri',
  'tests/prd-gate/fixtures/cross_sub_geometry_ref.ri',
];

describe('reify.grammar corpus — slice A (structure header)', () => {
  for (const relPath of SLICE_A) {
    it(`parses ${relPath} with zero error nodes`, () => {
      expect(countErrorNodes(readFixture(relPath))).toBe(0);
    });
  }
});

describe('reify.grammar snippets — structure header', () => {
  it('parses `structure def Foo { }`', () => {
    expect(countErrorNodes('structure def Foo { }')).toBe(0);
  });

  it('parses `pub structure def Foo { }`', () => {
    expect(countErrorNodes('pub structure def Foo { }')).toBe(0);
  });

  // `def` is optional in tree-sitter grammar.js:504-515 and many committed
  // prd-gate fixtures still use the bare form — it must keep parsing.
  it('parses the back-compat `structure Foo { ... }` form (no `def`)', () => {
    expect(countErrorNodes('structure Foo { param a : Length = 1mm }')).toBe(0);
  });
});
