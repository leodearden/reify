import { describe, it, expect } from 'vitest';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { highlightTree, classHighlighter } from '@lezer/highlight';
import { parser } from '../editor/reifyParser.js';
import { reifyLRLanguage } from '../editor/reifyLanguage';
import { KEYWORDS } from '../editor/highlight';

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
function countErrorNodes(src: string): number {
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

/** Node types that group two operands under an infix operator. */
const OPERATOR_NODES = new Set(['BinaryExpression', 'RangeExpression']);

/**
 * Source text of the LEFT OPERAND of the OUTERMOST infix-operator node in
 * `src` — the discriminator for operator-precedence assertions.
 *
 * `a and b or c` grouped as `(a and b) or c` yields `'a and b'`; grouped the
 * other way it would yield `'a'`. A zero-error-node check cannot tell those
 * apart (identical node multisets), and a node-name check cannot either, so
 * precedence claims are pinned through here. Cursor order is outermost-first,
 * so the first node reached from `OPERATOR_NODES` is the root of the operator
 * tree.
 *
 * The set must list EVERY infix node type, not just `BinaryExpression`:
 * matching one type alone silently descends past a differently-named root and
 * reports an inner operand, which reads as a precedence failure when the
 * grouping is in fact correct. `RangeExpression` is a separate node type
 * precisely because grammar.js models ranges as their own rule.
 */
function leftOperandOf(src: string): string {
  const cursor = parser.parse(src).cursor();
  do {
    if (OPERATOR_NODES.has(cursor.type.name)) {
      const left = cursor.node.firstChild;
      if (!left) throw new Error(`outermost ${cursor.type.name} has no children`);
      return src.slice(left.from, left.to);
    }
  } while (cursor.next());
  throw new Error(`no infix-operator node in parse of: ${src}`);
}

/**
 * ANTI-VACUITY GUARD. Every other assertion in this file is of the form
 * `countErrorNodes(...) === 0` or "this path is in the clean set". If the
 * helper ever silently degraded to always returning 0 — a @lezer/lr change to
 * `cursor.type.isError`, or `tree.cursor()` gaining a default that skips
 * anonymous/error nodes — the whole suite would stay green while covering
 * nothing, and the drift ledger would happily report 329/329 clean. This pins
 * that the helper can still report a non-zero count.
 */
describe('reify.grammar — countErrorNodes helper', () => {
  it('reports error nodes on input that cannot parse', () => {
    // Measured: 3 error nodes.
    expect(countErrorNodes('@@@ !!! ???')).toBeGreaterThan(0);
  });

  it('reports zero on input that parses', () => {
    expect(countErrorNodes('structure def Foo { }')).toBe(0);
  });
});

/**
 * Companion anti-vacuity guard for `leftOperandOf`. Pinned against the
 * SYMBOLIC operator bands, which predate this file and are not touched by the
 * keyword-band work below — so if the helper ever stopped discriminating
 * groupings, these fail independently of whatever the keyword band does.
 */
describe('reify.grammar — leftOperandOf helper', () => {
  it('reports the whole left subtree when the left operator binds tighter', () => {
    expect(leftOperandOf('structure def F { constraint a * b + c }')).toBe('a * b');
  });

  it('reports only the leaf when the right operator binds tighter', () => {
    expect(leftOperandOf('structure def F { constraint a + b * c }')).toBe('a');
  });
});

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

  it('parses a module declaration `module company.products.actuators`', () => {
    expect(countErrorNodes('module company.products.actuators')).toBe(0);
  });

  it('parses the back-compat legacy form `import "foo.ri"`', () => {
    expect(countErrorNodes('import "foo.ri"')).toBe(0);
  });

  // NOTE — the destructured import form (`import a.b {C, D}` vs
  // `import a.b.{C, D}`) is deliberately NOT asserted here. The tree-sitter
  // RULE at grammar.js:258-282 sequences the path and the items with no
  // separator, but grammar.js's own doc comment on that rule (:263) and the
  // lowering at crates/reify-syntax/src/ts_parser.rs:538 both spell it with a
  // dot. Nothing settles the disagreement: `import_items` has no tree-sitter
  // corpus test, no committed `.ri` uses the form, and no Rust code matches on
  // the node name. Asserting either spelling here would cement an unverified
  // shape as an intentional GUI contract, so the production stays (faithful to
  // the rule) and the test does not pin it. See the ImportDeclaration comment
  // in reify.grammar; a follow-up resolves the canonical form.

  /**
   * `module` is admitted ONLY at the top of `SourceFile`, never as a member of
   * `Declaration`, so the top-of-file / one-per-file rule falls out of the
   * grammar with no extra code (mirroring grammar.js:126-132). That is a
   * normative claim, so it is pinned by assertion rather than left in a
   * comment — per docs/legibility/design-invariants.md.
   */
  it('rejects a module declaration placed after another declaration', () => {
    // Measured: 2 error nodes.
    expect(countErrorNodes('structure def A { }\nmodule a.b')).toBeGreaterThan(0);
  });

  it('rejects a second module declaration in the same file', () => {
    // Measured: 2 error nodes.
    expect(countErrorNodes('module a\nmodule b')).toBeGreaterThan(0);
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
 * Named arguments inside CALL argument lists (`f(a: 1mm)`), transliterated
 * from tree-sitter-reify/grammar.js:1542-1546.
 *
 * `NamedArgument` already existed for `SubDeclaration`'s instantiation form,
 * but `ArgumentList` admitted only bare `expression`, so every call passing a
 * named argument produced error nodes. Measured before the fix:
 * `structure def F { let x = f(a: 1mm) }` → 2 error nodes. This is the largest
 * single lever in the corpus — 98 of the 239 then-failing `.ri` files contain
 * a named call argument.
 */
describe('reify.grammar snippets — named call arguments', () => {
  it('parses a single named argument `FEAMaterialInput(material: material)`', () => {
    expect(
      countErrorNodes('structure def F { let mi = FEAMaterialInput(material: material) }'),
    ).toBe(0);
  });

  it('parses multiple named arguments `Material(name: "steel", density: 7850)`', () => {
    expect(
      countErrorNodes('structure def F { let m = Material(name: "steel", density: 7850) }'),
    ).toBe(0);
  });

  // Widening ArgumentList must not cost the positional form: both arms start
  // with `Identifier`, so a mis-resolved conflict would break plain calls.
  it('still parses positional arguments `point3(0, 1, 2)`', () => {
    expect(countErrorNodes('structure def F { let p = point3(0, 1, 2) }')).toBe(0);
  });

  it('parses the mixed positional-then-named form `f(1, a: 2)`', () => {
    expect(countErrorNodes('structure def F { let x = f(1, a: 2) }')).toBe(0);
  });

  it('parses a trailing comma after a named argument', () => {
    expect(countErrorNodes('structure def F { let x = f(a: 1mm,) }')).toBe(0);
  });

  // Pin the SHAPE, not merely the absence of errors: an `ArgumentList` that
  // swallowed `a: 1mm` as some other construct would also report zero errors.
  it('yields a NamedArgument node for the named form', () => {
    expect(nodeNames('structure def F { let x = f(a: 1mm) }')).toContain('NamedArgument');
  });
});

/**
 * Collection literals and index access, transliterated from
 * tree-sitter-reify/grammar.js:1557-1566 (`list_literal`, `set_literal`,
 * `map_literal`, `map_entry`) and :1578-1583 (`index_access`).
 *
 * Measured before the fix: `structure def F { let x = [1, 2] }` → 3 error
 * nodes, because the grammar had no `[` token at all. List literals appear in
 * 83 of the 239 then-failing files and index access in 50 — the second-largest
 * lever after named call arguments, and inseparable from it because the same
 * files use both.
 */
describe('reify.grammar snippets — collection literals and indexing', () => {
  it('parses a list literal `[1, 2]`', () => {
    expect(countErrorNodes('structure def F { let x = [1, 2] }')).toBe(0);
  });

  it('parses the empty list `[]`', () => {
    expect(countErrorNodes('structure def F { let x = [] }')).toBe(0);
  });

  it('parses a trailing comma in a list `[1, 2,]`', () => {
    expect(countErrorNodes('structure def F { let x = [1, 2,] }')).toBe(0);
  });

  it('parses a nested list `[[1], [2]]`', () => {
    expect(countErrorNodes('structure def F { let x = [[1], [2]] }')).toBe(0);
  });

  // Corpus-attested (examples/assembly_rollup.ri): a list literal is a primary
  // expression, so `.sum` must still reach it through MemberAccess.
  it('parses a member access on a list literal `[a.b, c.d].sum`', () => {
    expect(
      countErrorNodes('structure def F { let total = [self.a.cost, self.b.cost].sum }'),
    ).toBe(0);
  });

  it('parses index access `pts[0]`', () => {
    expect(countErrorNodes('structure def F { let a = pts[0] }')).toBe(0);
  });

  it('parses a chained index access `m.rows[i]`', () => {
    expect(countErrorNodes('structure def F { let a = m.rows[i] }')).toBe(0);
  });

  it('parses an index into a call result `f(x)[0]`', () => {
    expect(countErrorNodes('structure def F { let a = f(x)[0] }')).toBe(0);
  });

  it('parses a set literal `set { 1, 2 }`', () => {
    expect(countErrorNodes('structure def F { let s = set { 1, 2 } }')).toBe(0);
  });

  // Corpus-attested (tests/prd-gate/fixtures/expected_type_pushdown_let.ri).
  it('parses the empty set literal `set {}`', () => {
    expect(countErrorNodes('structure def F { let s : Set<Length> = set {} }')).toBe(0);
  });

  it('parses a map literal `map { 1 => 2 }`', () => {
    expect(countErrorNodes('structure def F { let m = map { 1 => 2 } }')).toBe(0);
  });

  // Corpus-attested spelling (examples/load_case.ri): no space before `{`.
  it('parses the tight map form `map{"a" => 1, "b" => 2}`', () => {
    expect(countErrorNodes('structure def F { let raw = map{"a" => 1, "b" => 2} }')).toBe(0);
  });

  it('yields ListLiteral and IndexAccess nodes', () => {
    const names = nodeNames('structure def F { let a = [1, 2][0] }');
    expect(names).toContain('ListLiteral');
    expect(names).toContain('IndexAccess');
  });

  it('yields SetLiteral, MapLiteral and MapEntry nodes', () => {
    expect(nodeNames('structure def F { let s = set { 1 } }')).toContain('SetLiteral');
    const mapNames = nodeNames('structure def F { let m = map { 1 => 2 } }');
    expect(mapNames).toContain('MapLiteral');
    expect(mapNames).toContain('MapEntry');
  });

  // `set` and `map` must leave the ReservedWord @specialize list to become
  // `kw<>` productions (lezer-generator permits one replacement name per
  // (base token, literal) pair). Guard the consequence: a Block still opens
  // with `{` after a plain declaration header, i.e. the new `kw<"set"> "{"`
  // sequence did not capture the brace.
  it('still parses a plain block after promoting `set`/`map`', () => {
    expect(countErrorNodes('structure def F { param a : Length = 1mm }')).toBe(0);
  });
});

/**
 * The keyword logical-operator band — `not`, `and`, `or`, `implies` —
 * transliterated from tree-sitter-reify/grammar.js:1347-1352 (binary) and
 * :1391-1397 (unary).
 *
 * Measured before the fix: `structure def F { constraint not a }` produced a
 * `ReservedWord` node plus an error node, because the four words were listed
 * in the `ReservedWord` @specialize list and so parsed only as bare operands,
 * never as operators. `constraint not fouls` is a real first-error line in the
 * committed corpus.
 */
describe('reify.grammar snippets — keyword logical operators', () => {
  it('parses the unary `constraint not a`', () => {
    expect(countErrorNodes('structure def F { constraint not a }')).toBe(0);
  });

  it('parses `constraint a and b`', () => {
    expect(countErrorNodes('structure def F { constraint a and b }')).toBe(0);
  });

  it('parses `constraint a or b`', () => {
    expect(countErrorNodes('structure def F { constraint a or b }')).toBe(0);
  });

  it('parses `constraint a implies b`', () => {
    expect(countErrorNodes('structure def F { constraint a implies b }')).toBe(0);
  });

  it('parses the mixed-precedence `constraint a and b or c`', () => {
    expect(countErrorNodes('structure def F { constraint a and b or c }')).toBe(0);
  });

  // The symbolic operators are kept for back-compat (grammar.js:1354-1355,
  // deprecation deferred per PRD §10 Q3), so the two bands must coexist.
  it('parses keyword and symbolic operators together `a && b or c`', () => {
    expect(countErrorNodes('structure def F { constraint a && b or c }')).toBe(0);
  });

  it('parses `not` applied to a comparison `constraint not (a < b)`', () => {
    expect(countErrorNodes('structure def F { constraint not (a < b) }')).toBe(0);
  });

  /**
   * Precedence is a normative claim (spec §16: `and` level 13 binds tighter
   * than `or` level 14), so it is pinned by assertion rather than left to the
   * `@precedence` block's declaration order — a zero-error-node check cannot
   * tell the two groupings apart, since both contain exactly the same nodes.
   *
   * `leftOperandOf` reads the LEFT OPERAND of the outermost binary expression,
   * which is what discriminates them and is independent of how the operator
   * node ends up named.
   */
  it('binds `and` tighter than `or`', () => {
    expect(leftOperandOf('structure def F { constraint a and b or c }')).toBe('a and b');
  });

  it('binds `and` tighter than `implies`', () => {
    expect(leftOperandOf('structure def F { constraint a and b implies c }')).toBe('a and b');
  });

  // `not` is unary and binds looser than every symbolic operator, so it takes
  // the whole comparison as its operand rather than just `a`.
  it('applies `not` to the whole comparison in `not a < b`', () => {
    expect(countErrorNodes('structure def F { constraint not a < b }')).toBe(0);
  });
});

/**
 * Range expressions, transliterated from tree-sitter-reify/grammar.js:1376-1389
 * — two-sided `a .. b`, exclusive-upper `a ..< b`, and the four single-sided
 * prefix forms.
 *
 * Measured before the fix: `let r = 1mm .. 5mm` produced 2 error nodes,
 * because `..` lexed as two member-access dots. 59 of the then-failing files
 * contain `..` and 19 a prefix range.
 *
 * Every shape below is attested in a committed fixture: the joint-limit form
 * in examples/kinematic/counter_mass_balance.ri:18, the exclusive form in
 * examples/integration_full_v01.ri:157, and all four prefix forms in
 * examples/single_sided_range.ri:17-26.
 */
describe('reify.grammar snippets — range expressions', () => {
  it('parses a two-sided range `0rad .. 6.28rad`', () => {
    expect(countErrorNodes('structure def F { let turn = 0rad .. 6.283185307179586rad }')).toBe(0);
  });

  it('parses a range as a call argument `prismatic(vec3(1, 0, 0), 0mm .. 500mm)`', () => {
    expect(
      countErrorNodes('structure def F { let j = prismatic(vec3(1, 0, 0), 0mm .. 500mm) }'),
    ).toBe(0);
  });

  it('parses the space-free form `5mm..5mm`', () => {
    expect(countErrorNodes('structure def F { let r = 5mm..5mm }')).toBe(0);
  });

  it('parses a negative lower bound `-180deg .. 180deg`', () => {
    expect(countErrorNodes('structure def F { let r = revolute(axis_y, -180deg .. 180deg) }')).toBe(
      0,
    );
  });

  it('parses the exclusive-upper form `0..<5`', () => {
    expect(countErrorNodes('structure def F { let r = 0..<5 }')).toBe(0);
  });

  for (const [name, form] of [
    ['>', '>2mm'],
    ['>=', '>=2mm'],
    ['<', '<100mm'],
    ['<=', '<=100mm'],
  ]) {
    it(`parses the single-sided prefix range \`${name}\``, () => {
      expect(countErrorNodes(`structure def F { let b = ${form} }`)).toBe(0);
    });
  }

  it('yields a RangeExpression node', () => {
    expect(nodeNames('structure def F { let r = 1mm .. 5mm }')).toContain('RangeExpression');
    expect(nodeNames('structure def F { let b = >2mm }')).toContain('RangeExpression');
  });

  /**
   * grammar.js:1374-1375 pins range at precedence 0 — LOWER than every other
   * binary operator — so `2mm + 1mm .. 10mm - 1mm` is `(2mm+1mm)..(10mm-1mm)`
   * and not `2mm + (1mm..10mm) - 1mm`. Pinned by reading the left operand,
   * since both groupings contain the same nodes.
   */
  it('binds `..` looser than the arithmetic operators', () => {
    const src = 'structure def F { let r = 2mm + 1mm .. 10mm - 1mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('2mm + 1mm');
  });

  /**
   * THE HIGHEST-RISK INTERACTION IN THIS SLICE. The prefix range and binary
   * comparison share their leading operator token, so a mis-resolved conflict
   * turns every `a < b` into `a` followed by a prefix range. This is the
   * regression guard.
   */
  it('still parses `constraint thickness < 50mm` as a comparison, not a prefix range', () => {
    const src = 'structure def F { param thickness : Length = 5mm\n  constraint thickness < 50mm }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('CompareOp');
    expect(names).not.toContain('RangeExpression');
  });

  it('still parses a chained comparison-free member access `a.b.c`', () => {
    // `..` must not swallow the single `.` of member access.
    expect(countErrorNodes('structure def F { let x = a.b.c }')).toBe(0);
  });
});

/**
 * Type parameters and trait bounds on declaration headers, transliterated from
 * tree-sitter-reify/grammar.js:478-503 (`trait_bound_list`,
 * `trait_bound_entry`, `type_parameters`, `type_parameter`) and wired into
 * `structure_definition` (:504-515) and `occurrence_definition` (:517-528).
 *
 * Measured before the fix: `structure def CapScrew : Costed { }` produced
 * error nodes. Trait bounds appear in 50 of the then-failing files, type
 * parameters in 16, and `structure def CapScrew : Costed {` /
 * `structure def Bracket : Costed + Massive {` are both real first-error
 * lines. Every header below is attested in the committed corpus.
 */
describe('reify.grammar snippets — type parameters and trait bounds', () => {
  it('parses a single trait bound `structure def CapScrew : Costed { }`', () => {
    expect(countErrorNodes('structure def CapScrew : Costed { }')).toBe(0);
  });

  it('parses a multi-bound header `: Costed + Massive`', () => {
    expect(countErrorNodes('structure def Bracket : Costed + Massive { }')).toBe(0);
  });

  it('parses a three-bound header `: Physical + Elastic + Strong`', () => {
    expect(countErrorNodes('structure def AluminumBracket : Physical + Elastic + Strong { }')).toBe(
      0,
    );
  });

  it('parses a parameterized bound `: Container<T>`', () => {
    expect(countErrorNodes('structure def P : Container<T> { }')).toBe(0);
  });

  it('parses a bare type-parameter header `structure def Box<T> { }`', () => {
    expect(countErrorNodes('structure def Box<T> { }')).toBe(0);
  });

  // Corpus-attested (examples/): `structure def Bearing<T: Seal> {`.
  it('parses a bounded type parameter `<T: Seal>`', () => {
    expect(countErrorNodes('structure def Bearing<T: Seal> { }')).toBe(0);
  });

  // Corpus-attested: `structure def ActuatorInterface<P: PowerPort, T: ThermalPort, F: FluidPort>`.
  it('parses multiple bounded type parameters', () => {
    expect(
      countErrorNodes(
        'structure def ActuatorInterface<P: PowerPort, T: ThermalPort, F: FluidPort> { }',
      ),
    ).toBe(0);
  });

  it('parses a defaulted type parameter `<T = Length>`', () => {
    expect(countErrorNodes('structure def Box<T = Length> { }')).toBe(0);
  });

  it('parses type parameters and a bound together `<T> : Costed`', () => {
    expect(countErrorNodes('structure def Box<T> : Costed { }')).toBe(0);
  });

  it('parses an occurrence definition with a bound', () => {
    expect(countErrorNodes('occurrence def Weld : Physical { }')).toBe(0);
  });

  it('parses a bare occurrence definition', () => {
    expect(countErrorNodes('occurrence def Weld { param a : Length = 1mm }')).toBe(0);
  });

  it('yields TypeParameters and TraitBoundList nodes', () => {
    const names = nodeNames('structure def Box<T : Costed> : Massive { }');
    expect(names).toContain('TypeParameters');
    expect(names).toContain('TraitBoundList');
  });

  // Regression: the header `<`/`>` must not steal the comparison operators,
  // and a plain header with no bound must still parse.
  it('still parses a plain header and a body comparison', () => {
    expect(countErrorNodes('structure def F { constraint a < b }')).toBe(0);
  });
});

/**
 * `sub` declaration forms beyond the instantiation arm.
 *
 * The Lezer grammar admits only `sub Name = Struct(args)`. grammar.js:805-899
 * defines three arms, and the two missing ones dominate the corpus: `sub
 * idlers : List<Pulley>` is the single most frequent first-error line in the
 * whole 329-file corpus (9 files), and 41 failing files use some `sub … :`
 * form.
 *
 * Every shape below is lifted from a committed `.ri` — the `priv` prefix is
 * the one exception and is called out where it appears.
 */
describe('reify.grammar snippets — sub declaration forms', () => {
  // Corpus-attested 9x, e.g. examples/ (`sub idlers : List<Pulley>`) — the
  // most frequent single first-error line in the corpus.
  it('parses the collection form `sub idlers : List<Pulley>`', () => {
    expect(countErrorNodes('structure def F { sub idlers : List<Pulley> }')).toBe(0);
  });

  // Corpus-attested: `sub plate : Plate`, `sub c : Carriage`, `sub base : Base`.
  it('parses the bare specialization form `sub c : C`', () => {
    expect(countErrorNodes('structure def F { sub c : C }')).toBe(0);
  });

  // Corpus-attested: examples/auto_binding_sites.ri:59.
  it('parses a specialization with a body `sub b : Bearing { bore = auto }`', () => {
    expect(countErrorNodes('structure def F { sub b : Bearing { bore = auto } }')).toBe(0);
  });

  // Corpus-attested: examples/objective_inheritance.ri:26 (`sub c : C {}`).
  it('parses a specialization with an empty body `sub c : C {}`', () => {
    expect(countErrorNodes('structure def F { sub c : C {} }')).toBe(0);
  });

  // The type-argument arm of grammar.js:882-899. Attested in the corpus as
  // `sub vents : Keyed<Vent> { … }`; asserted here without the keyed body,
  // which is a separate member-block form out of this slice's scope.
  it('parses the type-argument form `sub p : P<Length>`', () => {
    expect(countErrorNodes('structure def F { sub p : P<Length> }')).toBe(0);
  });

  // Corpus-attested 2x: `sub bolt : Bolt at auto`, plus `sub link : Link at auto`.
  it('parses the pose clause `sub bolt : Bolt at auto`', () => {
    expect(countErrorNodes('structure def F { sub bolt : Bolt at auto }')).toBe(0);
  });

  // Corpus-attested: `sub shaft : Shaft at transform3(orient_identity(), vec3(100mm, 0mm, 0mm))`.
  it('parses a pose EXPRESSION clause `at transform3(…)`', () => {
    expect(
      countErrorNodes(
        'structure def F { sub shaft : Shaft at transform3(orient_identity(), vec3(100mm, 0mm, 0mm)) }',
      ),
    ).toBe(0);
  });

  // Corpus-attested on the collection arm: `sub ps : List<P> at transform3(…)`.
  it('parses a pose clause on the collection form', () => {
    expect(
      countErrorNodes(
        'structure def F { sub ps : List<P> at transform3(orient_identity(), vec3(0mm, 0mm, 0mm)) }',
      ),
    ).toBe(0);
  });

  // Corpus-attested 4x: `aux sub spare : HexBolt`, `aux sub jig : Jig`, … .
  it('parses the `aux` prefix', () => {
    expect(countErrorNodes('structure def F { aux sub spare : HexBolt }')).toBe(0);
  });

  /**
   * `priv` is the ONE shape in this block with no corpus occurrence. It is
   * normative rather than invented — grammar.js:808/833/882 carries
   * `optional('priv')` on all three arms — and it reaches the parser through
   * the identical optional-prefix mechanism as the attested `aux`, so it
   * cannot be doomed-by-construction the way an unattested novel shape could.
   */
  it('parses the `priv` prefix (normative, unattested in the corpus)', () => {
    expect(countErrorNodes('structure def F { priv sub hidden : Widget }')).toBe(0);
  });

  it('parses `priv aux` together', () => {
    expect(countErrorNodes('structure def F { priv aux sub h : Widget }')).toBe(0);
  });

  /**
   * REGRESSION GUARD. The instantiation arm is the one form the grammar
   * already accepted, and all three arms share the leading `sub Identifier`
   * prefix — they diverge only on the `=` vs `:` that follows. A mis-resolved
   * conflict there would silently break the form that works today.
   */
  it('still parses the instantiation form `sub s = Screw(len: 10mm)`', () => {
    expect(countErrorNodes('structure def F { sub s = Screw(len: 10mm) }')).toBe(0);
  });

  it('still parses the instantiation form with a where guard', () => {
    expect(countErrorNodes('structure def F { sub s = Screw(len: 10mm) where len > 1mm }')).toBe(0);
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
  /**
   * DATA-DRIVEN GUARD for FUTURE promotions. The per-word assertions below
   * only cover the words promoted in this change; a later promotion (say `fn`
   * or `trait` moving out of the ReservedWord list into a `kw<>` production)
   * would drop out of the `ReservedWord: t.keyword` rule, render unstyled, and
   * leave the enumerated tests green. So instead of trusting the enumeration,
   * extract every `kw<"…">` literal straight from the grammar source and
   * require each to be a member of the KEYWORDS array that highlight.ts joins
   * into its styleTags selector.
   */
  it('styles every `kw<>` production the grammar declares', () => {
    const grammarSrc = readFixture('gui/src/editor/reify.grammar');
    const declared = [...grammarSrc.matchAll(/kw<"([A-Za-z_][A-Za-z0-9_]*)">/g)].map((m) => m[1]);
    // Sanity: the extraction found something, so an empty match set cannot
    // make the subset check below pass vacuously.
    expect(declared.length).toBeGreaterThan(10);

    const styled = new Set(KEYWORDS);
    const unstyled = [...new Set(declared)].filter((word) => !styled.has(word)).sort();
    expect(
      unstyled,
      `These words are \`kw<>\` productions in reify.grammar but are missing from ` +
        `KEYWORDS in gui/src/editor/highlight.ts, so they render unstyled in the ` +
        `editor. Add them to KEYWORDS in the same commit as the promotion.`,
    ).toEqual([]);
  });

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

  it('styles `set` and `map` as keywords', () => {
    expect(keywordSpans('structure def F { let s = set { 1 } }')).toContain('set');
    expect(keywordSpans('structure def F { let m = map { 1 => 2 } }')).toContain('map');
  });

  /**
   * The logical operators are the semantically surprising promotions — they
   * read as operators, so it is easy to reach for `t.operator` (as `ArithOp`
   * and `CompareOp` do) and never notice they dropped out of `t.keyword`. The
   * data-driven guard above would catch a missing KEYWORDS entry; these pin
   * that the styling actually reaches the token in operator POSITION, which it
   * would not if the word were matched by some other production.
   */
  it('styles the keyword logical operators', () => {
    expect(keywordSpans('structure def F { constraint not a }')).toContain('not');
    expect(keywordSpans('structure def F { constraint a and b }')).toContain('and');
    expect(keywordSpans('structure def F { constraint a or b }')).toContain('or');
    expect(keywordSpans('structure def F { constraint a implies b }')).toContain('implies');
  });

  it('still styles a word that remains in the ReservedWord list', () => {
    expect(keywordSpans('structure def F { let x = trait }')).toContain('trait');
  });
});

// ── Corpus drift ledger ──────────────────────────────────────────────────
//
// The GUI Lezer grammar is a hand-maintained subset of the authoritative
// tree-sitter-reify/grammar.js. Nothing in CI previously noticed when the
// language moved ahead of it, so the editor degraded silently. This ledger
// makes that drift visible.
//
// MEASUREMENT (task 5907). The reference prototype for this task was measured
// over a NON-RECURSIVE walk of the two corpus roots — 146 + 71 = 217 files —
// and moved them from 7 clean to 57 clean (39 examples + 18 prd-gate
// fixtures). This ledger walks both roots RECURSIVELY, which picks up
// subdirectories such as examples/auto/ and examples/best_practices/ for a
// total of 329 files. The delta landed here measures:
//
//   top-level only : 57 / 217  (39 examples + 18 fixtures) — reproduces the
//                              reference prototype exactly
//   recursive      : 90 / 329  (the 57 above, plus 33 in subdirectories)
//
// The remaining files need expression- and declaration-level work explicitly
// out of this task's scope (prefix ranges, `^`, index access, lambdas, `@`
// selectors, string interpolation, and the
// fn/enum/trait/port/connect/chain/forall/match/unit/occurrence/purpose/field
// declaration families).
//
// WHY A PINNED SET AND NOT A COUNT. An absolute floor (`clean.length >= 90`)
// is coupled to corpus INVENTORY, not to grammar capability: deleting or
// renaming a currently-clean `.ri` — a routine refactor with no grammar
// involvement — would fail with "the grammar regressed", a false accusation
// whose obvious fix (lower the floor) also masks any genuine regression
// landing alongside it. Conversely, adding 50 unparseable examples would keep
// a count green while coverage rotted. So the ledger pins the SET instead:
// every path below is expected to parse clean, filtered by what still exists
// on disk. Removals drop out naturally, a genuine regression names the exact
// file that stopped parsing, and a coverage gain is a visible one-line
// addition here — the ratchet a follow-up grammar task turns.
const EXPECTED_CLEAN = [
  'examples/affine_tapered_spacer.ri',
  'examples/appearance_viewport_egress.ri',
  'examples/aspect_massive.ri',
  'examples/best_practices/bolt_circle.ri',
  'examples/best_practices/clearance_oracle.ri',
  'examples/best_practices/hollow_primitives.ri',
  'examples/best_practices/negation.ri',
  'examples/best_practices/symmetry_mirror.ri',
  'examples/bom_lifecycle.ri',
  'examples/bracket.ri',
  'examples/buckling_column_p2.ri',
  'examples/buckling_column_smoke.ri',
  'examples/buckling_multi_case_smoke.ri',
  'examples/complex_abs_arg.ri',
  'examples/complex_div.ri',
  'examples/complex_literals.ri',
  'examples/complex_numbers.ri',
  'examples/complex_transcendental.ri',
  'examples/conditional_compilation/platform_linux.ri',
  'examples/conditional_compilation/platform_wasm.ri',
  'examples/cost_aggregation.ri',
  'examples/cost_subtree_aggregate.ri',
  'examples/datum_projections.ri',
  'examples/dimensional_chains.ri',
  'examples/dimensional_consistency.ri',
  'examples/dimensionless_unification.ri',
  'examples/dynamics/closed_2prismatic_idyn.ri',
  'examples/dynamics/closed_4bar_idyn.ri',
  'examples/dynamics/pendulum_idyn.ri',
  'examples/dynamics/toolhead_motor_sizing.ri',
  'examples/error_messages.ri',
  'examples/extrude_infinite.ri',
  'examples/fdm_bracket.ri',
  'examples/fea_bracket_member_access.ri',
  'examples/fea_cantilever_smoke.ri',
  'examples/fea_multi_case_bracket.ri',
  'examples/fea_multi_case_smoke.ri',
  'examples/fea_pressure_smoke.ri',
  'examples/fea_shell_flexure.ri',
  'examples/fea_shell_too_thick_annotated.ri',
  'examples/fea_shell_too_thick_auto.ri',
  'examples/fields/from_samples.ri',
  'examples/fields/spatial_ops.ri',
  'examples/flexures/cantilever_beam_prb.ri',
  'examples/flexures/double_parallelogram.ri',
  'examples/flexures/notch_hinge_circular_prb.ri',
  'examples/flexures/parallelogram_stage.ri',
  'examples/flexures/printer_z_compliant_mount.ri',
  'examples/flexures/yield_warning.ri',
  'examples/gdt_conformance_satisfied.ri',
  'examples/gdt_conformance_violated.ri',
  'examples/geometric_relations/feature_datum_axis.ri',
  'examples/half_space.ri',
  'examples/io_formats.ri',
  'examples/kernel_queries/adjacent_faces.ri',
  'examples/kernel_queries/angle_smoke.ri',
  'examples/kernel_queries/box_edges.ri',
  'examples/kernel_queries/box_faces.ri',
  'examples/kernel_queries/contains_box.ri',
  'examples/kernel_queries/curvature_smoke.ri',
  'examples/kernel_queries/directional_selectors.ri',
  'examples/kernel_queries/distance_box_point.ri',
  'examples/kernel_queries/filtered_edges.ri',
  'examples/kernel_queries/geo_equiv_smoke.ri',
  'examples/kernel_queries/intersects_smoke.ri',
  'examples/kernel_queries/length_perimeter.ri',
  'examples/kernel_queries/normal_smoke.ri',
  'examples/kinematic/four_bar_singular.ri',
  'examples/kinematic/revolute_pivot_offset.ri',
  'examples/kinematic/spatial_linkage_oriented.ri',
  'examples/linalg.ri',
  'examples/litter_tray.ri',
  'examples/load_case.ri',
  'examples/m10_geometric_types.ri',
  'examples/m5_geometry.ri',
  'examples/m6_fallback_recovery.ri',
  'examples/m6_result_fallback.ri',
  'examples/m8_tolerancing.ri',
  'examples/m8_units.ri',
  'examples/m9_determinacy.ri',
  'examples/material_appearance_library.ri',
  'examples/materials_starter_library.ri',
  'examples/math_linalg.ri',
  'examples/modal/cantilever_beam_modes.ri',
  'examples/modal/printer_gantry_modes.ri',
  'examples/modal/simply_supported_beam_modes.ri',
  'examples/modal/transient_step_response.ri',
  'examples/module_visibility/consumer.ri',
  'examples/module_visibility/mismatch_variant.ri',
  'examples/multi_aspect_objective_mixed.ri',
  'examples/multi_kernel/manifold_boolean.ri',
  'examples/multi_kernel/voxel_to_mesh.ri',
  'examples/multi_kernel/voxel_to_mesh_iso.ri',
  'examples/multi_pane_viewport.ri',
  'examples/numeric_and_range_literals.ri',
  'examples/numeric_separators.ri',
  'examples/parametric_rate_cross_module.ri',
  'examples/parametric_vec3_cross_module.ri',
  'examples/pattern_composition.ri',
  'examples/perforated_plate.ri',
  'examples/process/std_process_dfm.ri',
  'examples/process/std_process_dfm_metrology.ri',
  'examples/process/std_process_dfm_thickness.ri',
  'examples/radix_literals.ri',
  'examples/selectors/relational_selectors_v2.ri',
  'examples/selectors/selector_vocabulary_v2_leaves.ri',
  'examples/selectors/single_face_by_normal.ri',
  'examples/selectors/vertices_index_coercion.ri',
  'examples/shells/thin_walled_bracket.ri',
  'examples/single_sided_range.ri',
  'examples/solid-param-direct.ri',
  'examples/solver_optimality_unproven.ri',
  'examples/stdlib/constants.ri',
  'examples/stdlib/process.ri',
  'examples/structural_query_children_members.ri',
  'examples/structural_query_descendants.ri',
  'examples/structure-instance.ri',
  'examples/sub_placement_assembly.ri',
  'examples/surface_finish_viewport.ri',
  'examples/sweep_degenerate.ri',
  'examples/tensegrity_cable_net.ri',
  'examples/tensegrity_membrane_formfind.ri',
  'examples/tensegrity_membrane_patch.ri',
  'examples/tensegrity_t_prism.ri',
  'examples/tolerancing/gdt_illegal_modifier.ri',
  'examples/tolerancing/gdt_legality_rfs.ri',
  'examples/tolerancing/gdt_oracle_inside.ri',
  'examples/tolerancing/gdt_oracle_outside.ri',
  'examples/tolerancing/gdt_removed_2018.ri',
  'examples/tolerancing/gdt_zones.ri',
  'examples/tolerancing/std_tolerancing_surface.ri',
  'examples/tolerancing/vc_bolt_pattern_clearance.ri',
  'examples/tolerancing/vc_bolt_pattern_interference.ri',
  'examples/tolerancing/vc_boundary_solid.ri',
  'examples/trajectory/ei_robustness.ri',
  'examples/trajectory/gcode_import_smoke.ri',
  'examples/trajectory/printer_print_envelope.ri',
  'examples/trajectory/tots_optimal_ptp.ri',
  'examples/trajectory/zv_shaped_ramp.ri',
  'examples/trajectory/zvd_robustness.ri',
  'examples/undef_self_describing.ri',
  'tests/prd-gate/fixtures/bare_angle_silently_accepted.ri',
  'tests/prd-gate/fixtures/collection_expr_index_resolves.ri',
  'tests/prd-gate/fixtures/collection_sub_at_placement_rejected.ri',
  'tests/prd-gate/fixtures/collection_sub_member_cell_consumable.ri',
  'tests/prd-gate/fixtures/collection_sub_per_member_cells.ri',
  'tests/prd-gate/fixtures/collection_sub_value_position_undef_baseline.ri',
  'tests/prd-gate/fixtures/compiler_type_hygiene_mul_scale_guard_defeat.ri',
  'tests/prd-gate/fixtures/compiler_type_hygiene_mul_vec_silent_int.ri',
  'tests/prd-gate/fixtures/cost_min_money_objective.ri',
  'tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri',
  'tests/prd-gate/fixtures/cross_sub_geometry_ref.ri',
  'tests/prd-gate/fixtures/dcr_dimension_rejection_channel_fires.ri',
  'tests/prd-gate/fixtures/dcr_langsurface_crossdim_silent.ri',
  'tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_shaper_frequency_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_solver_load_dropped_bare.ri',
  'tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri',
  'tests/prd-gate/fixtures/expected_type_pushdown_let.ri',
  'tests/prd-gate/fixtures/faces_by_normal_symbolic_eval_silent.ri',
  'tests/prd-gate/fixtures/geometry_let_selector_consumer.ri',
  'tests/prd-gate/fixtures/geometry_let_selector_consumer_edit.ri',
  'tests/prd-gate/fixtures/hand_placed_twin_two_subs_eval.ri',
  'tests/prd-gate/fixtures/indexed_sub_bare_member_resolves.ri',
  'tests/prd-gate/fixtures/indexed_sub_coll_arm_baseline.ri',
  'tests/prd-gate/fixtures/indexed_sub_inst_arm_baseline.ri',
  'tests/prd-gate/fixtures/indexed_sub_oob_computed_silent_undef.ri',
  'tests/prd-gate/fixtures/indexed_sub_oob_literal_silent_undef.ri',
  'tests/prd-gate/fixtures/indexed_sub_self_member_misrouted.ri',
  'tests/prd-gate/fixtures/indexed_sub_self_member_nogeom_unsupported.ri',
  'tests/prd-gate/fixtures/indexed_sub_silent_undef_baseline.ri',
  'tests/prd-gate/fixtures/indexed_sub_spec_arm_baseline.ri',
  'tests/prd-gate/fixtures/ir_clean_eval.ri',
  'tests/prd-gate/fixtures/objective_inherit_ambiguous.ri',
  'tests/prd-gate/fixtures/posed_subs_distance_query_unresolvable.ri',
  'tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri',
  'tests/prd-gate/fixtures/revolute_silent_accept.ri',
  'tests/prd-gate/fixtures/self_collection_count_redirect_rejected.ri',
  'tests/prd-gate/fixtures/single_sub_pose_resolves.ri',
  'tests/prd-gate/fixtures/stdlib_ns_buckling_mode_coexist.ri',
  'tests/prd-gate/fixtures/stdlib_ns_mode_member.ri',
  'tests/prd-gate/fixtures/stdlib_ns_mode_member_modal.ri',
  'tests/prd-gate/fixtures/stdlib_ns_std_nonexistent_import.ri',
  'tests/prd-gate/fixtures/stdlib_units_import_resolves.ri',
  'tests/prd-gate/fixtures/subbody_objective_ignored.ri',
  'tests/prd-gate/fixtures/transform3_unresolved.ri',
  'tests/prd-gate/fixtures/uncons_box_no_error.ri',
  'tests/prd-gate/fixtures/unit_nm_torque_immediate.ri',
];

const CORPUS_ROOTS = ['examples', 'tests/prd-gate/fixtures'];

function collectRiFiles(relDir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(join(REPO_ROOT, relDir), { withFileTypes: true })) {
    const rel = `${relDir}/${entry.name}`;
    if (entry.isDirectory()) out.push(...collectRiFiles(rel));
    else if (entry.name.endsWith('.ri')) out.push(rel);
  }
  return out;
}

// Wall-clock for the ledger below. The walk itself is fast — 329 files parse
// in ~0.75 s when this file runs alone — but under the full suite (159 test
// files across parallel workers) it is CPU-starved and overran vitest's 15 s
// default, so the timeout is raised explicitly rather than left to chance.
const LEDGER_TIMEOUT_MS = 120_000;

describe('reify.grammar — corpus drift ledger', () => {
  it('still parses every committed .ri that is expected to parse clean', () => {
    // The walk and all ~329 parses live INSIDE the `it`, not in the describe
    // body: a throw at collection time (a fixture removed mid-walk, an
    // unreadable file, a pathological input) would otherwise take down every
    // unrelated slice and snippet test in this file with an error that does
    // not even name the offending path.
    const allFiles = CORPUS_ROOTS.flatMap(collectRiFiles).sort();
    const cleanSet = new Set(
      allFiles.filter((p) => {
        // A file that cannot be read or parsed counts as not-clean rather than
        // aborting the suite.
        try {
          return countErrorNodes(readFixture(p)) === 0;
        } catch {
          return false;
        }
      }),
    );

    // Paths that have since been deleted or renamed drop out silently — this
    // ledger tracks grammar capability, not corpus inventory.
    const stillPresent = EXPECTED_CLEAN.filter((p) => existsSync(join(REPO_ROOT, p)));
    const regressed = stillPresent.filter((p) => !cleanSet.has(p));

    expect(
      regressed,
      `These committed .ri files parsed with zero error nodes but no longer do — ` +
        `the grammar regressed:\n${regressed.map((p) => `  ${p}`).join('\n')}\n` +
        `(measured ${cleanSet.size} clean of ${allFiles.length} committed .ri files; ` +
        `${stillPresent.length} of the ${EXPECTED_CLEAN.length} pinned paths still exist on disk)`,
    ).toEqual([]);
  }, LEDGER_TIMEOUT_MS);
});
