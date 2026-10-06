import { describe, it, expect } from 'vitest';
import {
  countErrorNodes,
  countNodesNamed,
  keywordSpans,
  nodeNamesSpanning,
  sourceOfNodeNamed,
} from './lezerProbeHelpers';

/**
 * The constrained-2d-sketch editor surface on the GUI Lezer grammar
 * (PRD constrained-2d-sketch): the `pub`/`priv`/`aux` prefix of grammar.js
 * `let_declaration`, the `sketch_block` member, and the positional
 * `auto_seed` argument. Shapes are pinned by span plus count, never by an
 * error count alone.
 */

// ── `let` prefixes ──────────────────────────────────────────────────────────

describe('reify.grammar — `pub` / `priv` / `aux` prefixes on a let declaration', () => {
  it('absorbs `aux` into the LetDeclaration rather than leaving it beside it', () => {
    const src = 'structure def S { aux let cl = line(a, b) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'LetDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'Member')).toBe(1);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe('aux let cl = line(a, b)');
  });

  it.each(['pub let x = 1mm', 'priv let y = 2mm', 'priv aux let z = 3mm'])(
    'starts the LetDeclaration at the prefix of `%s`',
    (decl) => {
      const src = `structure def S { ${decl} }`;
      expect(countErrorNodes(src)).toBe(0);
      expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe(decl);
    },
  );

  it('admits the prefix in a constraint-definition body', () => {
    const src = 'constraint def C {\n  aux let x = 1\n  x > 0\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe('aux let x = 1');
    expect(countNodesNamed(src, 'ConstraintDefPredicate')).toBe(1);
  });

  it('admits the prefix in a port body', () => {
    const src = 'structure def S { port p : in F { aux let x = 1mm } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe('aux let x = 1mm');
  });

  it.each(['aux sub a = B()', 'priv aux sub a = B()'])(
    'still reads `%s` as one SubDeclaration',
    (decl) => {
      const src = `structure def S { ${decl} }`;
      expect(countErrorNodes(src)).toBe(0);
      expect(countNodesNamed(src, 'SubDeclaration')).toBe(1);
    },
  );

  it('still reads `priv param` as one ParamDeclaration', () => {
    const src = 'structure def S { priv param t : Real = 5 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'ParamDeclaration')).toBe(1);
  });
});

// ── Sketch block ────────────────────────────────────────────────────────────

/** `body` lines placed inside `sketch s { … }` inside a structure. */
function inSketch(...body: string[]): string {
  return `structure def S {\n  sketch s {\n${body.map((l) => `    ${l}\n`).join('')}  }\n}`;
}

describe('reify.grammar — sketch blocks', () => {
  it('reads entity lets and relation members as siblings with no separator', () => {
    const src =
      'structure def S {\n  sketch profile {\n    let a = point(0mm, 0mm)\n    fix(a)\n  }\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'SketchBlock')).toBe(1);
    expect(countNodesNamed(src, 'LetDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'RelationMember')).toBe(1);
  });

  it('admits an `aux let` entity in the body', () => {
    const decl = 'aux let cl = line(origin, point(0mm, 10mm))';
    const src = `structure def S { sketch profile { ${decl} } }`;
    expect(countErrorNodes(src)).toBe(0);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe(decl);
  });

  it('admits a type-annotated entity', () => {
    const src = 'structure def S { sketch s { let a : Point2 = point(0mm, 0mm) } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'LetDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'TypeAnnotation')).toBe(1);
    expect(sourceOfNodeNamed(src, 'TypeAnnotation')).toBe(': Point2');
  });

  it('admits an empty body', () => {
    const src = 'structure def S { sketch s { } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'SketchBlock')).toBe(1);
  });

  it.each([
    ['a guarded block', 'structure def S { where c { sketch s { fix(a) } } }'],
    ['a specialization body', 'structure def S { sub b : B { sketch s { fix(a) } } }'],
  ])('nests in %s', (_where, src) => {
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'SketchBlock')).toBe(1);
  });

  it('rejects a param in the body, which admits only lets and relations', () => {
    expect(countErrorNodes('structure def S { sketch s { param p : Length = 1mm } }')).toBeGreaterThan(0);
  });
});

// Parity with grammar.js's `sketch_body_item_boundary_matches_a_plain_member_body`:
// both readings of each pair parse clean, so the node counts and spans decide.
describe('reify.grammar — sketch body item boundaries', () => {
  it('keeps a let and a following call as two members', () => {
    const src = inSketch('let d = 5mm', 'fix(a)');
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'LetDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'RelationMember')).toBe(1);
  });

  it('keeps two relation calls as two members', () => {
    const src = inSketch('fix(a)', 'horizontal(ab)');
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'RelationMember')).toBe(2);
  });

  it('joins a namespaced ref and a parenthesised next line into one call', () => {
    const src = inSketch('let x = a.b', '(c)');
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'LetDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'RelationMember')).toBe(0);
    const joined = src.slice(src.indexOf('a.b'), src.indexOf('(c)') + '(c)'.length);
    expect(nodeNamesSpanning(src, joined)).toContain('NamespacedCall');
  });

  it('continues a let across a line that opens with a binary operator', () => {
    const src = inSketch('let d = 5mm', '- 3mm');
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'RelationMember')).toBe(0);
    const both = src.slice(src.indexOf('let d'), src.indexOf('- 3mm') + '- 3mm'.length);
    expect(sourceOfNodeNamed(src, 'LetDeclaration')).toBe(both);
  });
});

// `sketch` is a contextual `ekw<>`: a keyword only where a SketchBlock opens.
describe('reify.grammar — `sketch` is a contextual keyword', () => {
  it('styles `sketch` as a keyword where it opens a block', () => {
    expect(keywordSpans('structure def S { sketch s { fix(a) } }')).toContain('sketch');
  });

  it('leaves `sketch` an ordinary identifier as a let name', () => {
    const src = 'structure def S { let sketch = 1mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(keywordSpans(src)).not.toContain('sketch');
  });

  it('leaves `sketch` an ordinary identifier as an operand', () => {
    const src = 'structure def S { let y = sketch + 1 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'sketch')).toContain('Identifier');
    expect(keywordSpans(src)).not.toContain('sketch');
  });

  it('reads `sketch = 5mm` in a specialization body as a param assignment', () => {
    const src = 'structure def S { sub b : B { sketch = 5mm } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'ParamAssignment')).toBe(1);
    expect(countNodesNamed(src, 'SketchBlock')).toBe(0);
  });
});

// ── `auto(<expr>)` seed ─────────────────────────────────────────────────────

describe('reify.grammar — auto(<expr>) seed in positional argument position', () => {
  it('reads `auto(10mm)` as exactly one AutoSeed, never an AutoKeyword', () => {
    const src = 'structure def S { let b = point(auto(10mm), 0mm) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'auto(10mm)')).toEqual(['AutoSeed']);
    expect(countNodesNamed(src, 'AutoKeyword')).toBe(0);
  });

  it('admits the seed in a non-first position', () => {
    const src = 'structure def S { let b = f(x, auto(1mm)) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'AutoSeed')).toBe(1);
  });

  it.each([
    ['FunctionCall', 'f(auto(1mm))'],
    ['NamespacedCall', 'a.b(auto(1mm))'],
    ['TraitMethodCall', 'Foo::bar(auto(1mm))'],
    ['AdHocSelector', 'body @ faces(auto(1mm))'],
  ])('reaches the seed through the %s consumer', (consumer, call) => {
    const src = `structure def S { let b = ${call} }`;
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, consumer)).toBe(1);
    expect(countNodesNamed(src, 'AutoSeed')).toBe(1);
  });

  it('admits the seed inside a sketch body', () => {
    const src = inSketch(
      'let a = point(0mm, 0mm)',
      'let b = point(auto(10mm), 0mm)',
      'horizontal(ab)',
    );
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'AutoSeed')).toBe(1);
  });

  // grammar.js reads positional `auto(free)` as a seed whose value is the
  // identifier `free`; the free-modifier arm is unreachable in argument position.
  it.each([
    ['a structure let', 'structure def S { let b = f(auto(free)) }'],
    ['a sketch body', 'structure def S { sketch s { let b = point(auto(free), 0mm) } }'],
  ])('reads positional `auto(free)` in %s as one AutoSeed, never an AutoKeyword', (_where, src) => {
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'auto(free)')).toContain('AutoSeed');
    expect(countNodesNamed(src, 'AutoSeed')).toBe(1);
    expect(countNodesNamed(src, 'AutoKeyword')).toBe(0);
  });

  // Each boundary below matches grammar.js, which keeps these readings.
  it('still rejects bare positional `auto` (task 3808)', () => {
    expect(countErrorNodes('structure def S { let x = f(auto) }')).toBeGreaterThan(0);
  });

  it('still rejects the named-parameter auto form in positional position', () => {
    expect(countErrorNodes('structure def S { let x = f(auto(seed = 5mm)) }')).toBeGreaterThan(0);
  });

  it('does not admit the seed at a binding site', () => {
    expect(countErrorNodes('structure def S { let x : Length = auto(5mm) }')).toBeGreaterThan(0);
  });

  it('keeps a named-argument `auto(free)` an AutoKeyword', () => {
    const src = 'structure def S { let x = f(x: auto(free)) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'AutoKeyword')).toBe(1);
    expect(countNodesNamed(src, 'AutoSeed')).toBe(0);
  });
});
