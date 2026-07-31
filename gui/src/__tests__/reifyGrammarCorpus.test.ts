import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { highlightTree, classHighlighter } from '@lezer/highlight';
import { parser } from '../editor/reifyParser.js';
import { reifyLRLanguage } from '../editor/reifyLanguage';

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

/** Parse `src` and collect the set of node type names in the resulting tree. */
function nodeNames(src: string): Set<string> {
  const tree = parser.parse(src);
  const cursor = tree.cursor();
  const names = new Set<string>();
  do {
    names.add(cursor.type.name);
  } while (cursor.next());
  return names;
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

/**
 * SLICE B — committed fixtures that additionally need the module/import delta.
 */
const SLICE_B = [
  // top-of-file `module` declaration + chained `a.b.c.d` member access
  'examples/material_appearance_library.ri',
  // dotted `import std.units`
  'tests/prd-gate/fixtures/stdlib_units_import_resolves.ri',
  // `module` + `minimize` + `constraint thickness < 50mm` — the last doubles as
  // the regression guard that `<`/`>` still parse as comparison operators.
  'tests/prd-gate/fixtures/cost_min_money_objective.ri',
];

describe('reify.grammar corpus — slice A (structure header)', () => {
  for (const relPath of SLICE_A) {
    it(`parses ${relPath} with zero error nodes`, () => {
      expect(countErrorNodes(readFixture(relPath))).toBe(0);
    });
  }
});

describe('reify.grammar corpus — slice B (module + import)', () => {
  for (const relPath of SLICE_B) {
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

describe('reify.grammar snippets — module and import', () => {
  it('parses an aliased import `import parts as pp`', () => {
    expect(countErrorNodes('import parts as pp')).toBe(0);
  });

  it('parses a dotted import `import std.units`', () => {
    expect(countErrorNodes('import std.units')).toBe(0);
  });

  it('parses a pub aliased dotted import `pub import a.b as c`', () => {
    expect(countErrorNodes('pub import a.b as c')).toBe(0);
  });

  // tree-sitter grammar.js:258-282 sequences import_path then import_items with
  // NO separator — `import a.b.{C, D}` is not the language's form.
  it('parses a destructured import `import a.b {C, D}`', () => {
    expect(countErrorNodes('import a.b {C, D}')).toBe(0);
  });

  it('parses a module declaration `module company.products.actuators`', () => {
    expect(countErrorNodes('module company.products.actuators')).toBe(0);
  });

  it('parses the back-compat legacy form `import "foo.ri"`', () => {
    expect(countErrorNodes('import "foo.ri"')).toBe(0);
  });
});

/**
 * TypeExpr cases, each wrapped in a minimal structure so the assertion is on a
 * real declaration rather than a bare fragment. Shapes transliterated from
 * tree-sitter-reify grammar.js:1064-1140.
 */
const TYPE_EXPRS = [
  // parameterized
  'Box<T>',
  // multi-arg with an integer type-arg (Tensor<rank, n, quantity>)
  'Tensor<2, 3, Force>',
  // qualified
  'Beam::Material',
  // the FORK-G trait disambiguator (PRD §3.5 Phase 8)
  'Beam::(HasMaterial::Material)',
  // applied-base projection
  'Coupling<Prismatic>::MotionValue',
  // arrow / function types
  '(Length) -> Length',
  '(A, B) -> C',
  '() -> C',
];

describe('reify.grammar snippets — TypeExpr', () => {
  for (const typeExpr of TYPE_EXPRS) {
    it(`parses the type annotation \`${typeExpr}\``, () => {
      expect(countErrorNodes(`structure def F { param b : ${typeExpr} = auto }`)).toBe(0);
    });
  }

  it('still parses a bare-identifier annotation `param w : Length = 80mm`', () => {
    expect(countErrorNodes('structure def F { param w : Length = 80mm }')).toBe(0);
  });

  // Guard: `<` and `>` become type-argument delimiters in ParameterizedType.
  // They must keep working as comparison operators in expression position.
  it('still parses `constraint thickness < 50mm` as a comparison', () => {
    const src = 'structure def F { param thickness : Length = 5mm\n  constraint thickness < 50mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('CompareOp');
  });
});

/**
 * Drives `reifyLRLanguage` — the exact object the editor uses, already wired
 * with the `@external propSource` — through `highlightTree`, and collects the
 * source text of every span that received `t.keyword`.
 */
function keywordSpans(src: string): string[] {
  const tree = reifyLRLanguage.parser.parse(src);
  const spans: string[] = [];
  highlightTree(tree, classHighlighter, (from, to, classes) => {
    if (classes.split(' ').includes('tok-keyword')) spans.push(src.slice(from, to));
  });
  return spans;
}

/**
 * Promoting a word out of the `ReservedWord` @specialize list is mandatory
 * (lezer-generator otherwise hard-fails on a conflicting specialization), but
 * it silently drops that word out of the `ReservedWord: t.keyword` styleTags
 * rule — it becomes its own node that no selector names, so it renders
 * unstyled. That is precisely the "degrading syntax highlighting" symptom this
 * task is about, so it gets a real assertion instead of being left to review.
 */
describe('reify.grammar — keyword highlighting for promoted keywords', () => {
  it('styles `module` as a keyword', () => {
    expect(keywordSpans('module a.b')).toContain('module');
  });

  it('styles `pub` and `as` as keywords', () => {
    const spans = keywordSpans('pub import a.b as c');
    expect(spans).toContain('pub');
    expect(spans).toContain('as');
    // control: `import` was never de-specialized and must still be styled
    expect(spans).toContain('import');
  });

  it('styles `def` as a keyword', () => {
    expect(keywordSpans('pub structure def Foo { param w : Length = 1mm }')).toContain('def');
  });

  it('still styles the untouched `structure` and `param` keywords', () => {
    const spans = keywordSpans('pub structure def Foo { param w : Length = 1mm }');
    expect(spans).toContain('structure');
    expect(spans).toContain('param');
  });

  it('still styles a word that remains in the ReservedWord list', () => {
    expect(keywordSpans('structure def F { let x = trait }')).toContain('trait');
  });
});
