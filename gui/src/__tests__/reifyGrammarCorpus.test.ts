import { describe, it, expect } from 'vitest';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { highlightTree, classHighlighter } from '@lezer/highlight';
import { foldNodeProp, indentNodeProp } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import type { SyntaxNode } from '@lezer/common';
import { parser } from '../editor/reifyParser.js';
import {
  reifyLRLanguage,
  BRACE_FIRST_BODIES,
  KEYWORD_LED_BODIES,
} from '../editor/reifyLanguage';
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

/**
 * Parse `src` and count the nodes named `name`.
 *
 * `nodeNames` collapses to a set, so a production that emits the SAME node
 * type nested inside itself reads as one hit and the redundancy is invisible.
 * That is not hypothetical: naming a rule and the token it wraps identically
 * made every wildcard arm come out as `WildcardPattern > WildcardPattern`
 * while `toContain('WildcardPattern')` stayed green (#5957). Use this wherever
 * the tree SHAPE, not just the presence of a name, is the contract.
 */
function countNodesNamed(src: string, name: string): number {
  const tree = parser.parse(src);
  const cursor = tree.cursor();
  let count = 0;
  do {
    if (cursor.type.name === name) count++;
  } while (cursor.next());
  return count;
}

/**
 * Node type names of every node whose span is EXACTLY the first occurrence of
 * `text` in `src`, outermost first — the discriminator for "what did this token
 * reduce TO?".
 *
 * `countNodesNamed(src, 'X') === 0` answers only what a token is NOT, and stays
 * green for every misparse that drops the token from the tree, absorbs it into
 * its parent with no child of its own, or brings it back under a third name.
 * Naming what actually spans the token separates those from the intended tree.
 * MEASURED: `A | _` gives `['Identifier']`; bare `_ => …` gives
 * `['MatchPattern', 'WildcardPattern']`. Throws rather than returning `[]` when
 * `text` is absent, so a typo'd needle fails loudly instead of vacuously.
 */
function nodeNamesSpanning(src: string, text: string): string[] {
  const from = src.indexOf(text);
  if (from < 0) throw new Error(`no ${JSON.stringify(text)} in: ${src}`);
  const to = from + text.length;
  const cursor = parser.parse(src).cursor();
  const names: string[] = [];
  do {
    if (cursor.from === from && cursor.to === to) names.push(cursor.type.name);
  } while (cursor.next());
  return names;
}

/**
 * Source text SPANNED by the first node named `name` — the discriminator for
 * "was this token absorbed into that node, or left beside it?".
 *
 * A count cannot answer that. An optional leading keyword parsed as a stray
 * SIBLING leaves the count of the node it should have joined at exactly 1, so
 * `countNodesNamed(..., 'ParamDeclaration') === 1` passes on both the right
 * tree and the wrong one; only the node's `from` offset separates them. Throws
 * rather than returning `''` when the node is absent, so a typo'd name fails
 * loudly instead of vacuously.
 */
function sourceOfNodeNamed(src: string, name: string): string {
  const cursor = parser.parse(src).cursor();
  do {
    if (cursor.type.name === name) return src.slice(cursor.from, cursor.to);
  } while (cursor.next());
  throw new Error(`no ${name} node in parse of: ${src}`);
}

/**
 * Node types that group two operands under an infix operator.
 *
 * Kept EXHAUSTIVE on purpose — the reason is spelled out on `leftOperandOf`
 * below. Beyond the two arithmetic/comparison shapes, the qualifying forms are
 * postfix-or-infix ones whose FIRST child is a left operand a precedence
 * assertion may need to name: `AdHocSelector` (`expr @ sel(args)`),
 * `IndexAccess` (`expr [ expr ]`), `MapEntry` (`expr => expr`),
 * `MemberAccess` (`expr . Identifier`) and round 4's three `::` forms —
 * `QualifiedAccess` (`Base :: name`), `InstanceQualifiedAccess`
 * (`expr . ( Base :: name )`) and `TraitMethodCall` (either of those plus a
 * call tail).
 *
 * The `::` three are listed here because they NEST — `TraitMethodCall` wraps an
 * `InstanceQualifiedAccess` which wraps a `QualifiedAccess` — so omitting them
 * armed exactly the trap `leftOperandOf`'s docstring records. MEASURED with all
 * three absent: `leftOperandOf('structure def F { let x = f.(A::b)(i + 1) }')`
 * descended past all three roots into the ARGUMENT's `BinaryExpression` and
 * returned `'i'`. The anti-vacuity block below covers one case per shape.
 */
const OPERATOR_NODES = new Set([
  'BinaryExpression',
  'RangeExpression',
  'AdHocSelector',
  'IndexAccess',
  'MapEntry',
  'MemberAccess',
  'QualifiedAccess',
  'InstanceQualifiedAccess',
  'TraitMethodCall',
]);

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
 *
 * That is not a hypothetical. MEASURED while `IndexAccess` was still missing
 * from the set: `leftOperandOf('structure def F { let x = a[i + 1] }')` walked
 * straight past the `IndexAccess` root, reached the inner `BinaryExpression`
 * and returned `'i'` — a confidently wrong answer rather than a throw. Adding
 * a node type to the grammar therefore means adding it to `OPERATOR_NODES` in
 * the same change, and the anti-vacuity block below covers one case per shape
 * so a future omission has somewhere to fail.
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

  /**
   * One case per NON-`BinaryExpression` member of `OPERATOR_NODES`, so a
   * future edit that adds an infix node type and forgets the set has an
   * existing shape to break. Each also pins a real grouping claim.
   *
   * The selector case is the one that pays twice: grammar.js:1568-1575 gives
   * `ad_hoc_selector` `prec.left(10)`, and left-associativity is observable
   * ONLY through a chain — the chained-selector snippet elsewhere in this file
   * asserts zero error nodes, which the right-associative grouping would
   * satisfy just as well.
   */
  it('reports the inner selector when a selector chain is left-associative', () => {
    expect(leftOperandOf('structure def F { let e = body @ edge("top") @ nearest(p) }')).toBe(
      'body @ edge("top")',
    );
  });

  /**
   * The index case is the measured regression named in the docstring above:
   * with `IndexAccess` absent from the set this returned the inner `'i'`.
   */
  it('reports the indexed base, not an operand of the index expression', () => {
    expect(leftOperandOf('structure def F { let x = a[i + 1] }')).toBe('a');
  });

  it('reports the key of a map entry', () => {
    expect(leftOperandOf('structure def F { let m = map { k + 1 => v } }')).toBe('k + 1');
  });

  it('reports the receiver of a dotted member access', () => {
    expect(leftOperandOf('structure def F { let m = self.b.bore }')).toBe('self.b');
  });

  /**
   * The `::` three, added in catch-up round 4. They nest, so one snippet per
   * shape is not enough — the DEEPEST one has to be reached through the other
   * two, which is the case that measured `'i'` while all three were missing
   * from the set. Each also pins a real grouping claim: the callee of a trait
   * method call is the whole qualified access, not its base.
   */
  it('reports the whole qualified callee of a trait method call', () => {
    expect(leftOperandOf('structure def F { let x = f.(A::b)(i + 1) }')).toBe('f.(A::b)');
    expect(leftOperandOf('structure def F { let x = A::b(i + 1) }')).toBe('A::b');
  });

  it('reports the receiver of a bare instance-qualified access', () => {
    expect(leftOperandOf('structure def F { let x = f.(A::b) }')).toBe('f');
  });

  it('reports the base of a bare qualified access', () => {
    expect(leftOperandOf('structure def F { let x = A::b }')).toBe('A');
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
  // in reify.grammar; #5931 resolves the canonical form.

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
 * Composite unit expressions (`7850kg/m^3`) and the value-level `^`.
 *
 * These are TWO independent features that share one character, and grammar.js
 * separates them by CONTIGUITY, not by symbol:
 *
 *   "unit_expr pow fires only when ^ is adjacent (no whitespace), while
 *    value-level ^ fires in the normal whitespace-permitted context"
 *                                             — grammar.js:1505-1507
 *
 * Both readings are attested in committed code: `7850kg/m^3` (22 files) is the
 * unit form, and `(5mm ^ 2) / (1mm ^ 2)` (examples/unit_expressions.ri:23) is
 * the spaced value-level form. 32 failing files use a unit `^` or `/`, and
 * `let steel_density = 7850kg/m^3` currently mis-parses as
 * `QuantityLiteral ArithOp Identifier ⚠ ⚠ Number`.
 */
describe('reify.grammar snippets — unit expressions and exponent', () => {
  // Corpus-attested 22x — the single most common composite unit in the corpus.
  it('parses `7850kg/m^3`', () => {
    expect(countErrorNodes('structure def F { let d = 7850kg/m^3 }')).toBe(0);
  });

  // Corpus-attested 2x.
  it('parses `9.81m/s^2` (fractional mantissa)', () => {
    expect(countErrorNodes('structure def F { let a = 9.81m/s^2 }')).toBe(0);
  });

  // Corpus-attested: `5N*m`, `5kN*m`.
  it('parses the multiplicative form `5N*m`', () => {
    expect(countErrorNodes('structure def F { let t = 5N*m }')).toBe(0);
  });

  // Corpus-attested 2x: `5N*m/rad`, and `1kg*m/s^2`.
  it('parses a mixed `*` and `/` unit `5N*m/rad`', () => {
    expect(countErrorNodes('structure def F { let k = 5N*m/rad }')).toBe(0);
  });

  // Corpus-attested: `0.5W/(m*K)`, `880.0J/(kg*K)`, `30.0W/(m*K)`.
  it('parses a parenthesised unit denominator `0.5W/(m*K)`', () => {
    expect(countErrorNodes('structure def F { let c = 0.5W/(m*K) }')).toBe(0);
  });

  // Corpus-attested 2x: `0.001kg/m/s` — chained division, no parens.
  it('parses a chained division unit `0.001kg/m/s`', () => {
    expect(countErrorNodes('structure def F { let mu = 0.001kg/m/s }')).toBe(0);
  });

  // Corpus-attested: examples/surface_finish_cost.ri:85 (`50USD/m^2`).
  it('parses a currency rate unit `50USD/m^2`', () => {
    expect(countErrorNodes('structure def F { let r = 50USD/m^2 }')).toBe(0);
  });

  /**
   * Normative but UNATTESTED: no committed file uses a negative unit exponent.
   * grammar.js:1531-1533 defines the exponent as
   * `signed_integer: token.immediate(/-?\d+/)`, so the minus sign is part of
   * the upstream token rather than something invented here.
   */
  it('parses a negative unit exponent `1m^-1` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { let x = 1m^-1 }')).toBe(0);
  });

  // ── Value-level `^`: the SPACED reading ──────────────────

  it('parses the value-level exponent `2 ^ 3`', () => {
    expect(countErrorNodes('structure def F { let x = 2 ^ 3 }')).toBe(0);
  });

  // Corpus-attested: examples/unit_expressions.ri:23 —
  // `param stress_sq : Real = (5mm ^ 2) / (1mm ^ 2)`. This is the exact shape
  // that proves the spaced reading is a real language form and not a
  // convenience invented for this test.
  it('parses the attested `(5mm ^ 2) / (1mm ^ 2)`', () => {
    expect(countErrorNodes('structure def F { param s : Real = (5mm ^ 2) / (1mm ^ 2) }')).toBe(0);
  });

  /**
   * `^` is right-associative at tree-sitter prec 8, so `2 ^ 3 ^ 2` is
   * `2 ^ (3 ^ 2)`. Both groupings contain identical nodes, so this is pinned
   * through the left operand: right-assoc yields `2`, left-assoc `2 ^ 3`.
   */
  it('binds `^` right-associatively', () => {
    const src = 'structure def F { let x = 2 ^ 3 ^ 2 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('2');
  });

  /**
   * prec 8 (`^`) > 7 (unary) > 6 (multiplicative), so `-2 ^ 2` is `-(2 ^ 2)`
   * and NOT `(-2) ^ 2`. Under the wrong grouping the outermost infix node
   * would carry the left operand `-2`.
   */
  it('binds `^` tighter than unary minus', () => {
    const src = 'structure def F { let x = -2 ^ 2 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('2');
  });

  it('binds `^` tighter than multiplication', () => {
    const src = 'structure def F { let x = 2 * 3 ^ 2 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('2');
  });

  /**
   * THE GREEDY-MATCH REGRESSION GUARD, and the reason step-14 keeps contiguity
   * in the TOKEN rather than in a parse rule.
   *
   * grammar.js polices unit contiguity with a C external scanner that peeks one
   * character past the operator and only enters the unit arm when what follows
   * can start a unit. Lezer has NO external scanner, so a widened
   * `QuantityLiteral` regex is the translation — and a regex that accepts any
   * character after `/` would swallow `25USD/1kg` whole, turning a binary
   * division into a single literal. `1` is a digit, not a unit start, so the
   * token must stop at `25USD` and leave `/ 1kg` to the expression grammar.
   */
  it('does NOT swallow `25USD/1kg` — division by a NUMBER stays binary', () => {
    const src = 'structure def F { let c = 25USD/1kg }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('25USD');
  });

  it('does NOT swallow a spaced division `25USD / 1kg`', () => {
    const src = 'structure def F { let c = 25USD / 1kg }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('25USD');
  });

  /**
   * Companion guard on the OTHER side: a plain identifier division must stay
   * binary too. `mass/volume` has a unit-start character after the `/`, so it
   * is the case a contiguity-only rule would most easily over-capture.
   */
  it('does NOT swallow an identifier division `mass/volume`', () => {
    const src = 'structure def F { let d = mass/volume }';
    expect(countErrorNodes(src)).toBe(0);
    expect(leftOperandOf(src)).toBe('mass');
  });
});

/**
 * `trait` declarations and `fn` definitions/signatures.
 *
 * Two of the declaration families this task names, and the two largest still
 * missing: `trait` appears in 35 of the then-failing files (`trait Measurable
 * {`, `trait Seal {}`, `trait FluidPort {` are all real first-error lines) and
 * `fn` in 14. Every shape below is attested — in a committed `.ri` where one
 * exists, otherwise in a tree-sitter corpus test, which is the authoritative
 * pin for the shapes no example happens to use.
 *
 * TWO SHAPES ARE DELIBERATELY NOT THE ONES THE PLAN PROSE SPELLED.
 *
 *   1. The fn-body `let` binding is asserted WITH its terminating `;` —
 *      `{ let a = 1.0; a }`, not `{ let a = 1.0 a }`. grammar.js:252-259 ends
 *      `fn_let_binding` with a required `';'`, and the upstream corpus test
 *      `tree-sitter-reify/test/corpus/function.txt:10` pins exactly
 *      `fn f(x: Int) -> Int { let y = x; y }`. Asserting the separator-free
 *      form would have forced a Lezer grammar strictly more permissive than
 *      the authoritative one — the editor would show no error for a program
 *      the compiler rejects. The normative source wins over the plan prose.
 *
 *   2. `AssociatedType` and `FunctionDefinition` are asserted inside a
 *      STRUCTURE body as well as inside a trait body. grammar.js:529-547 puts
 *      both in `_member` explicitly (`type X = Concrete` so a conformer can
 *      satisfy a trait's associated type; `fn f(self) -> T { … }` so it can
 *      override a default-providing associated fn), and
 *      examples/trait_assoc_type_material.ri:15 is a committed conformer that
 *      needs it. Trait-body-only would leave those files failing.
 */
describe('reify.grammar snippets — trait and fn declarations', () => {
  // Corpus-attested 6x: `trait Seal {}`, `trait Bolt {}`, `trait Flow {}`.
  it('parses the empty `trait Bolt {}`', () => {
    expect(countErrorNodes('trait Bolt {}')).toBe(0);
  });

  // Corpus-attested: examples/m5_combined_all.ri:4, examples/m9_integration.ri:56.
  it('parses a member-bearing trait `trait Measurable { param mass : Mass }`', () => {
    expect(countErrorNodes('trait Measurable { param mass : Mass }')).toBe(0);
  });

  // Corpus-attested: examples/m9_trait_conformance.ri:15 — a trait body carries
  // the same member forms a structure body does.
  it('parses a trait body with param, let and constraint members', () => {
    expect(
      countErrorNodes('trait Measurable { param size : Length let half = size / 2 constraint size > 0mm }'),
    ).toBe(0);
  });

  // Corpus-attested: `trait Left : Base {`, `trait Right : Base {`.
  it('parses the refined `trait Seal : Costed { }`', () => {
    expect(countErrorNodes('trait Seal : Costed { }')).toBe(0);
  });

  // Corpus-attested: examples/m9_trait_conformance.ri:54
  // (`trait Physical : Measurable + Weighable {`).
  it('parses a multi-bound refinement `: Measurable + Weighable`', () => {
    expect(countErrorNodes('trait Physical : Measurable + Weighable { }')).toBe(0);
  });

  // grammar.js:287 carries `optional($.type_parameters)` on the trait header,
  // the same production the structure header already uses.
  it('parses a parameterized `trait Container<T> { }`', () => {
    expect(countErrorNodes('trait Container<T> { }')).toBe(0);
  });

  it('parses a `pub trait`', () => {
    expect(countErrorNodes('pub trait Seal { }')).toBe(0);
  });

  // Corpus-attested: examples/trait_assoc_type_material.ri:12 (`type Material`).
  it('parses a required associated type `trait T { type Item }`', () => {
    expect(countErrorNodes('trait T { type Item }')).toBe(0);
  });

  // Corpus-attested: examples/trait_assoc_type_material.ri:15
  // (`type Material = Steel`), which sits in a STRUCTURE body — see the note
  // above on why `_member` admits it too.
  it('parses a defaulted associated type `trait T { type Item = Length }`', () => {
    expect(countErrorNodes('trait T { type Item = Length }')).toBe(0);
  });

  it('parses an associated-type binding in a STRUCTURE body', () => {
    expect(
      countErrorNodes('structure def Beam : HasMaterial { type Material = Steel param mass : Material }'),
    ).toBe(0);
  });

  // grammar.js:207-215 — the bodyless signature, reachable only via a trait
  // member. Corpus-attested shape: examples/trait_assoc_fn_static.ri declares
  // `fn make_default() -> Length` requirements in the same position.
  it('parses a bodyless signature `trait T { fn area() -> Area }`', () => {
    expect(countErrorNodes('trait T { fn area() -> Area }')).toBe(0);
  });

  // Corpus-attested: examples/trait_assoc_fn_cylinder.ri:43
  // (`fn lateral_area(self) -> Scalar<Area> { … }`) — the `self` receiver arm
  // of grammar.js:217-231.
  it('parses a `self` receiver `trait T { fn scale(self, k : Real) -> Length }`', () => {
    expect(countErrorNodes('trait T { fn scale(self, k : Real) -> Length }')).toBe(0);
  });

  it('parses a bare `self` receiver with a body', () => {
    expect(
      countErrorNodes('trait Cylindrical { fn lateral_area(self) -> Scalar<Area> { pi * diameter } }'),
    ).toBe(0);
  });

  // Corpus-attested: examples/generics/container.ri:7 — verbatim.
  it('parses the top-level `fn single<T>(x: T) -> List<T> { [x] }`', () => {
    expect(countErrorNodes('fn single<T>(x: T) -> List<T> { [x] }')).toBe(0);
  });

  // Corpus-attested: examples/m5_user_function.ri:1 (`fn area(w: Real, h: Real) -> Real { w * h }`).
  it('parses a two-parameter top-level fn', () => {
    expect(countErrorNodes('fn area(w: Real, h: Real) -> Real { w * h }')).toBe(0);
  });

  // Corpus-attested: examples/generics/dim_param.ri:15
  // (`fn scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }`).
  it('parses a bounded type parameter on a fn header', () => {
    expect(
      countErrorNodes('fn scale_q<Q: Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }'),
    ).toBe(0);
  });

  // grammar.js:239-246 expression-body arm, pinned upstream by
  // tree-sitter-reify/test/corpus/function.txt (`fn double(x: Int) -> Int = x * 2`).
  it('parses the expression-bodied `fn twice(x : Real) -> Real = x * 2`', () => {
    expect(countErrorNodes('fn twice(x : Real) -> Real = x * 2')).toBe(0);
  });

  // grammar.js:233-237 `optional(seq('=', field('default', …)))`, pinned
  // upstream by tree-sitter-reify/test/corpus/fn_param_default.txt.
  it('parses a defaulted parameter `fn f(x : Real = 1.0) -> Real = x`', () => {
    expect(countErrorNodes('fn f(x : Real = 1.0) -> Real = x')).toBe(0);
  });

  // The `;`-terminated block form — see note 1 above.
  it('parses fn-body let bindings `fn g() -> Real { let a = 1.0; a }`', () => {
    expect(countErrorNodes('fn g() -> Real { let a = 1.0; a }')).toBe(0);
  });

  // grammar.js:200 makes the return type optional.
  it('parses a fn with no return type', () => {
    expect(countErrorNodes('fn noop() { 1 }')).toBe(0);
  });

  it('parses a `pub fn`', () => {
    expect(countErrorNodes('pub fn area(w: Real) -> Real = w')).toBe(0);
  });

  // grammar.js:529-547 — the structure-body override arm of `_member`.
  it('parses an override fn inside a STRUCTURE body', () => {
    expect(
      countErrorNodes('structure def Pin : Cylindrical { fn lateral_area(self) -> Area { 1mm } }'),
    ).toBe(0);
  });

  it('yields TraitDeclaration, AssociatedType and FunctionDefinition nodes', () => {
    const names = nodeNames('trait T { type Item fn area() -> Area }');
    expect(names).toContain('TraitDeclaration');
    expect(names).toContain('AssociatedType');
    expect(names).toContain('FunctionDefinition');
  });

  /**
   * REGRESSION GUARD. `trait`, `fn`, `type` and `self` all leave the
   * `ReservedWord` @specialize list in this slice, and `type`'s `=` plus
   * `fn`'s `=`-bodied arm both collide with shapes the grammar already
   * accepts (`DefaultValue`, `ParamAssignment`, `LetDeclaration`). These pin
   * that the incumbent forms still parse.
   */
  it('still parses a structure body with param defaults and a sub', () => {
    expect(
      countErrorNodes('structure def F { param w : Length = 1mm sub s = Screw(len: 10mm) }'),
    ).toBe(0);
  });

  it('still parses a specialization body with a param assignment', () => {
    expect(countErrorNodes('structure def F { sub b : Bearing { bore = auto } }')).toBe(0);
  });

  /**
   * `self` IS THE ONE PROMOTION THAT HAD TO BE CONTEXTUAL, and these pin why.
   *
   * `self` appears 146 times in the committed corpus, every one of them in
   * EXPRESSION position (`constraint self.b.bore == 10mm`,
   * `[self.a.line_cost, self.b.line_cost].sum`). tree-sitter accepts those
   * because its lexer is parse-state-driven: `'self'` is a bare string used
   * only inside `fn_param_list`, so it keeps lexing as `identifier`
   * everywhere else. Lezer's `@specialize` is context-FREE, so a `kw<"self">`
   * would have reserved the word globally and turned every one of those lines
   * into an error node — which is why the receiver slot uses `ekw<"self">`.
   *
   * The drift ledger would catch that regression too, but only as "these 40
   * files stopped parsing"; these name the actual cause.
   */
  it('still parses expression-position `self` — `constraint self.b.bore == 10mm`', () => {
    expect(countErrorNodes('structure def F { constraint self.b.bore == 10mm }')).toBe(0);
  });

  it('still parses `self` inside a list literal member chain', () => {
    expect(
      countErrorNodes('structure def F { let total = [self.a.line_cost, self.b.line_cost].sum }'),
    ).toBe(0);
  });
});

/**
 * Lambda expressions and the `@` ad-hoc port selector.
 *
 * Both are named in this task's scope list. Measured: lambdas in 24 of the
 * failing files (`let doubled = sample(fn_field(|p| 2.0 * p), 3.0)` is a real
 * first-error line), `@` selectors in 13 (`let top_frame = post @ face("top")`).
 *
 * THE `||` COLLISION IS THE RISK IN THIS SLICE. A zero-parameter lambda opens
 * with two adjacent `|` characters, which is character-for-character the
 * symbolic OR operator the grammar has accepted since before this task. The
 * regression assertion at the bottom is the one that must never be relaxed:
 * `a || b` is attested throughout the corpus, a zero-parameter lambda is
 * attested nowhere in it.
 */
describe('reify.grammar snippets — lambdas and @ selectors', () => {
  // Corpus-attested: examples/fields/compose.ri:19 (`|x| 2.0 * x`).
  it('parses a single-parameter lambda `|x| x * 2`', () => {
    expect(countErrorNodes('structure def F { let f = |x| x * 2 }')).toBe(0);
  });

  // Corpus-attested: examples/option_map_or.ri:30
  // (`map_or(some(5mm), 0mm, |x: Length| x * 2.0)`).
  it('parses a typed lambda parameter `|x : Real| x * 2`', () => {
    expect(countErrorNodes('structure def F { let f = |x : Real| x * 2 }')).toBe(0);
  });

  /**
   * Multi-parameter and zero-parameter lambdas have NO corpus occurrence.
   * Both are normative — grammar.js:1222-1227 wraps the parameter list in
   * `commaSep`, which is `optional(seq(rule, repeat(...), optional(',')))`,
   * so zero, one and many are all admitted upstream.
   */
  it('parses a multi-parameter lambda `|a, b| a + b` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { let f = |a, b| a + b }')).toBe(0);
  });

  /**
   * The zero-parameter lambda is the collision case: `|| 1` and the OR operator
   * are the same two characters. Admitted upstream through `commaSep`'s empty
   * case (grammar.js:1222-1232), and admitted here through a second
   * `LambdaExpression` arm. Why an arm rather than a token precedence, and how
   * `RelateBlock` merged the `"|"` and `"||"` token groups in the first place,
   * are in the `||` collision note on `LambdaExpression` in reify.grammar.
   *
   * WHAT BELONGS HERE is why the assertion exists at all: it is the detector.
   * The merge compiled with ZERO conflicts and zero warnings and turned
   * `let f = || 1` into an error node in silence — the whole 330-file corpus
   * still passed, and this assertion plus the `a || b` guards below were the
   * only things that noticed. Token-group merging is not a conflict, so nothing
   * in the build reports one; there is no loud failure to fall back on.
   *
   * Pinned by NODE NAME, not by error count: each spelling still parses clean
   * under the wrong reading, so a count is blind to which one was chosen.
   */
  it('parses a zero-parameter lambda `|| 1` (normative, unattested)', () => {
    const src = 'structure def F { let f = || 1 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('LambdaExpression');
  });

  /**
   * The other side of the merge, and the one with 40-odd corpus occurrences:
   * `||` after a COMPLETE operand must still be the OR operator, never the
   * opener of a fresh lambda. Reachability does the work — the new arm is only
   * reachable where an expression may start — so this reads as a BinaryExpression.
   */
  it('still reads `a || b` as the OR operator, not a lambda opener', () => {
    const src = 'structure def F { constraint a || b }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('BinaryExpression');
    expect(names).not.toContain('LambdaExpression');
  });

  /**
   * The two spellings MEETING, which neither pin above reaches: a parameterized
   * lambda whose body contains the OR operator. `|x|` opens and closes with the
   * one-character token and `x || y` uses the two-character one, so this is the
   * single snippet in which both members of the merged group must be lexed
   * correctly in one parse — the shape most exposed to a future maximal-munch
   * change in that group, since a change to either spelling now moves both.
   */
  it('lexes both spellings in one parse: `|x| x || y`', () => {
    const src = 'structure def F { let f = |x| x || y }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('LambdaExpression');
    expect(names).toContain('LambdaParam');
    expect(names).toContain('BinaryExpression');
  });

  // Corpus-attested verbatim: examples/fields/fn_field.ri:14.
  it('parses a lambda as a call argument `sample(fn_field(|p| 2.0 * p), 3.0)`', () => {
    expect(countErrorNodes('structure def F { let d = sample(fn_field(|p| 2.0 * p), 3.0) }')).toBe(
      0,
    );
  });

  // Corpus-attested: examples/generate_bolt_circle.ri:16,28.
  it('parses a lambda in the trailing argument position `generate(n, |i| i * 2mm)`', () => {
    expect(countErrorNodes('structure def F { let ps = generate(n, |i| i * 2mm) }')).toBe(0);
  });

  // Corpus-attested: examples/generate_bolt_circle.ri:16-22 — the body spans
  // lines, which is the shape that proves the body extends as far right (and
  // as far down) as it can rather than stopping at the first newline.
  it('parses a lambda whose body spans several lines', () => {
    expect(
      countErrorNodes(
        'structure def F {\n  let ps = generate(n, |i|\n    point3(\n      r * cos(i),\n      r * sin(i),\n      0mm\n    )\n  )\n}',
      ),
    ).toBe(0);
  });

  it('yields a LambdaExpression node', () => {
    expect(nodeNames('structure def F { let f = |x| x * 2 }')).toContain('LambdaExpression');
  });

  // Corpus-attested verbatim: examples/multi_kernel/attribute_selectors.ri:78,
  // examples/ad_hoc_face_selector.ri:68.
  it('parses the ad-hoc selector `post @ face("top")`', () => {
    expect(countErrorNodes('structure def F { let top = post @ face("top") }')).toBe(0);
  });

  // Corpus-attested: examples/m10_combined.ri:88
  // (`let supply_point = supply @ point(0mm, 0mm, 0mm)`).
  it('parses a selector with several arguments `supply @ point(0mm, 0mm, 0mm)`', () => {
    expect(countErrorNodes('structure def F { let p = supply @ point(0mm, 0mm, 0mm) }')).toBe(0);
  });

  /**
   * Chaining has no corpus occurrence. It is normative rather than invented:
   * grammar.js:1568-1575 gives `ad_hoc_selector` `prec.left(10)`, and
   * left-associativity is only observable through a chain.
   */
  it('parses a chained selector (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { let e = body @ edge("top") @ nearest(p) }')).toBe(0);
  });

  // grammar.js puts the selector (10) tighter than index access (9) and looser
  // than member access (11), so a selector applied to a member-access base is
  // the interaction worth pinning.
  it('parses a selector on a member-access base `a.b @ face("top")`', () => {
    expect(countErrorNodes('structure def F { let t = a.b @ face("top") }')).toBe(0);
  });

  it('yields an AdHocSelector node', () => {
    expect(nodeNames('structure def F { let top = post @ face("top") }')).toContain('AdHocSelector');
  });

  /**
   * REGRESSION GUARD — the highest-risk interaction in this slice. `a || b`
   * must stay ONE symbolic OR operator and must not be re-read as a
   * zero-parameter lambda whose body is `b`. Zero error nodes alone cannot
   * tell those apart (the lambda reading also has none), so this asserts the
   * node shape and the operand grouping as well.
   */
  it('still parses `constraint a || b` as a single `||` operator', () => {
    const src = 'structure def F { constraint a || b }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).not.toContain('LambdaExpression');
    expect(leftOperandOf(src)).toBe('a');
  });

  it('still parses a chained `a || b || c`', () => {
    const src = 'structure def F { constraint a || b || c }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).not.toContain('LambdaExpression');
    expect(leftOperandOf(src)).toBe('a || b');
  });
});

/**
 * The parameterized `auto` forms and `auto_type_arg`.
 *
 * `auto_type_arg` is the one arm task 5907 explicitly deferred when it added
 * `ParameterizedType`/`TypeArgList`, and this task names it. Measured: 14
 * failing files use `auto(`, and `param thickness : Length = auto(free)` /
 * `param side : Real = auto(free)` are real first-error lines.
 *
 * The grammar today has `AutoKeyword { kw<"auto"> }` — the bare form only.
 */
describe('reify.grammar snippets — auto forms', () => {
  // Corpus-attested 9x: examples/continuous_cost_min.ri:62,
  // examples/best_practices/discrete_choice.ri:65, and seven more.
  it('parses `param thickness : Length = auto(free)`', () => {
    expect(countErrorNodes('structure def F { param thickness : Length = auto(free) }')).toBe(0);
  });

  // Corpus-attested: examples/interpolation.ri:33 — the same modifier on a
  // `let` rather than a `param`.
  it('parses `let gap : Length = auto(free)`', () => {
    expect(countErrorNodes('structure def F { let gap : Length = auto(free) }')).toBe(0);
  });

  /**
   * The PARAMETERIZED arm (`auto(name = value, …)`) has no corpus occurrence
   * — every committed `auto(` is `auto(free)`. It is normative:
   * grammar.js:653 gives `auto_keyword` a distinct `auto_param_list` arm, and
   * :660-668 documents exactly these value shapes (`5mm`, `self.frame`,
   * `orient_identity()`).
   */
  it('parses a parameterized auto `auto(seed = 5mm)` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { param p : Length = auto(seed = 5mm) }')).toBe(0);
  });

  it('parses a multi-entry parameterized auto `auto(x = 1mm, y = 2mm)`', () => {
    expect(countErrorNodes('structure def F { param p : Length = auto(x = 1mm, y = 2mm) }')).toBe(
      0,
    );
  });

  it('parses an expression-valued parameterized auto', () => {
    expect(
      countErrorNodes('structure def F { param p : Frame = auto(orientation = orient_identity()) }'),
    ).toBe(0);
  });

  // Corpus-attested: examples/auto/bearing_constraint_select.ri:80,
  // examples/auto/bearing_unsat.ri:93 (`sub bearing = Bearing<auto: Seal>()`).
  it('parses the auto type-arg `Bearing<auto: Seal>`', () => {
    expect(countErrorNodes('structure def F { sub bearing = Bearing<auto: Seal>() }')).toBe(0);
  });

  // Corpus-attested: examples/bearing_auto_seal.ri:78,
  // examples/auto/bearing_resolved_value.ri:58.
  it('parses the modified auto type-arg `Bearing<auto(free): Seal>`', () => {
    expect(countErrorNodes('structure def F { sub b = Bearing<auto(free): Seal>() }')).toBe(0);
  });

  // Corpus-attested: examples/auto/bounded_fallback_unsound.ri:102 — seven
  // auto type-args in one list.
  it('parses several auto type-args in one list', () => {
    expect(
      countErrorNodes('structure def F { sub s = LayeredStack<auto: Layer, auto: Layer>() }'),
    ).toBe(0);
  });

  // The plan's spelling of the same arm in a type ANNOTATION rather than a
  // sub instantiation — the `typeArg` rule is shared, so both reach it.
  it('parses an auto type-arg in a param annotation', () => {
    expect(countErrorNodes('structure def F { param b : Bearing<auto: Seal> = auto }')).toBe(0);
  });

  it('parses a modified auto type-arg in a param annotation', () => {
    expect(countErrorNodes('structure def F { param b : Bearing<auto(free): Seal> = auto }')).toBe(
      0,
    );
  });

  it('yields AutoTypeArg and AutoParam nodes', () => {
    expect(nodeNames('structure def F { sub b = Bearing<auto: Seal>() }')).toContain('AutoTypeArg');
    expect(nodeNames('structure def F { param p : Length = auto(seed = 5mm) }')).toContain(
      'AutoParam',
    );
  });

  /**
   * REGRESSION GUARD. The bare `auto` is the form the grammar already
   * accepted, and all three arms of grammar.js:647-651 share it as their
   * prefix — upstream needs explicit `prec(2)`/`prec(1)`/`prec(0)` to settle
   * the shift-reduce at `(`. A mis-resolved conflict here would break the
   * form that works today, in every position that admits it.
   */
  it('still parses the bare `param w : Length = auto`', () => {
    expect(countErrorNodes('structure def F { param w : Length = auto }')).toBe(0);
  });

  it('still parses a bare `auto` in a specialization body and a pose clause', () => {
    expect(countErrorNodes('structure def F { sub b : Bearing { bore = auto } }')).toBe(0);
    expect(countErrorNodes('structure def F { sub bolt : Bolt at auto }')).toBe(0);
  });
});

/**
 * The topology member families — `port`, `connect`, `chain`, `forall` — and
 * the `forall`/`exists` QUANTIFIER expression they share their keyword with.
 *
 * All four are `commonMembers()` entries upstream
 * (tree-sitter-reify/grammar.js:39-42), so they are admitted wherever a member
 * is: structure bodies, occurrence bodies, guarded blocks, specialization
 * bodies. Measured before this slice: `port` appears in 15 of the 239 failing
 * corpus files, `connect` in 8, `forall` in 4, `chain` in 2.
 *
 * Two collisions make this slice riskier than its file count suggests, and
 * each gets its own regression assertion at the bottom:
 *
 *  1. `<-` and `<->` share their first character with `<` and `<=`, so the
 *     longest-token `@precedence` idiom that arrow types and ranges already
 *     needed applies again. Without it `connect a <- b` lexes as `<` then `-`
 *     and `constraint a <= b` is unaffected — so the failure is silent in one
 *     direction and loud in the other.
 *  2. `direction` and `frame` are the port body's setting keywords upstream,
 *     but BOTH are attested as ordinary identifiers in committed `.ri`
 *     (`let direction = normalize(velocity)`, `param frame : Frame3 = …`,
 *     `frame: mid_f` as a named-argument label). A context-free `kw<>` would
 *     un-pin those files exactly as `at` did in the sub slice.
 */
describe('reify.grammar snippets — port, connect, chain, forall', () => {
  // ── Port ──────────────────────────────────────────────
  // The bodyless, directionless form. grammar.js:957-966 makes both the
  // direction and the body optional.
  it('parses a bare `port p : Fluid`', () => {
    expect(countErrorNodes('structure def F { port p : Fluid }')).toBe(0);
  });

  // Corpus-attested: examples/integration_corner_cases.ri:45.
  it('parses the bodyless directional `port output : out FullTrait`', () => {
    expect(countErrorNodes('structure def F { port output : out FullTrait }')).toBe(0);
  });

  it('parses the `in` direction `port p : in Fluid`', () => {
    expect(countErrorNodes('structure def F { port p : in Fluid }')).toBe(0);
  });

  /**
   * `bidi` is the third arm of `port_direction_keyword` (grammar.js:968) and
   * has ZERO occurrences in the committed corpus — it is admitted here on the
   * authoritative grammar alone. That is also why promoting it is safe: no
   * file can lose an identifier to the reservation.
   */
  it('parses the `bidi` direction `port p : bidi Fluid` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { port p : bidi Fluid }')).toBe(0);
  });

  // Corpus-attested: examples/auto_binding_sites.ri:105-106,
  // examples/keyed_vents.ri:23.
  it('parses an empty port body `port a : out EpsilonSignal {}`', () => {
    expect(countErrorNodes('structure def F { port a : out EpsilonSignal {} }')).toBe(0);
  });

  // Corpus-attested: examples/m5_connect_chain.ri:6.
  it('parses a port body holding a param declaration', () => {
    expect(
      countErrorNodes(
        'structure def F { port inlet : in FluidPort { param diameter : Length = 25mm } }',
      ),
    ).toBe(0);
  });

  // Corpus-attested: examples/integration_full_v01.ri:212-215 — a param
  // declaration and a `frame = …` setting in the same body.
  it('parses a port body holding a `frame = …` setting', () => {
    expect(
      countErrorNodes(
        'structure def F { port demand : in FluidInterface { param diameter : Length = 50mm frame = base_frame } }',
      ),
    ).toBe(0);
  });

  /**
   * `direction = …` inside a port body (grammar.js:982-986) is unattested —
   * every committed port states its direction in the header instead. Admitted
   * on the authoritative grammar, and paired with the `frame = self.f` setting
   * the plan names.
   */
  it('parses a port body holding `direction = in` and `frame = self.f`', () => {
    expect(
      countErrorNodes('structure def F { port p : Fluid { direction = in frame = self.f } }'),
    ).toBe(0);
  });

  it('yields a PortDeclaration node', () => {
    expect(nodeNames('structure def F { port p : in Fluid }')).toContain('PortDeclaration');
  });

  // ── Connect ───────────────────────────────────────────
  // Corpus-attested: examples/m10_combined.ri and 8 more.
  it('parses the forward connect `connect a.p -> b.q`', () => {
    expect(countErrorNodes('structure def F { connect a.p -> b.q }')).toBe(0);
  });

  /**
   * The reverse and bidirectional operators (grammar.js:1002) have no corpus
   * occurrence — every committed `connect` uses `->`. They are the reason the
   * `@precedence { "<->", "<-", "<=", "<" }` ordering is required, so they are
   * pinned here even though they are unattested.
   */
  it('parses the reverse connect `connect a.p <- b.q` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { connect a.p <- b.q }')).toBe(0);
  });

  it('parses the bidirectional connect `connect a.p <-> b.q` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { connect a.p <-> b.q }')).toBe(0);
  });

  // Corpus-attested: examples/m10_connect_advanced.ri — `: PipeConnector`.
  it('parses a typed connect `connect a.p -> b.q : Hose`', () => {
    expect(countErrorNodes('structure def F { connect a.p -> b.q : Hose }')).toBe(0);
  });

  // Corpus-attested: examples/m10_connect_advanced.ri — port mappings.
  it('parses a connect body of port mappings', () => {
    expect(
      countErrorNodes(
        'structure def F { connect outlet -> inlet { diameter -> diameter, flow_rate -> flow_rate } }',
      ),
    ).toBe(0);
  });

  // Corpus-attested: examples/auto_binding_sites.ri:108 — a connect param
  // assignment whose value is a solver-determined `auto`.
  it('parses a connect body of param assignments', () => {
    expect(
      countErrorNodes('structure def F { connect a -> b : EpsilonConnector { gain = auto } }'),
    ).toBe(0);
  });

  // Corpus-attested: examples/m10_connect_advanced.ri — mappings and
  // assignments mixed in one body.
  it('parses a connect body mixing param assignments and port mappings', () => {
    expect(
      countErrorNodes(
        'structure def F { connect source -> sink : BoltSet { grade = 10.9, diameter -> diameter, flow_rate -> flow_rate } }',
      ),
    ).toBe(0);
  });

  /**
   * Corpus-attested: examples/keyed_vents.ri:38. `port_ref` is a full
   * `_expression` upstream (grammar.js:1006), not a dotted name — this one
   * indexes a keyed collection before the member access, and is the assertion
   * that stops the port ref being narrowed to `Identifier ("." Identifier)*`.
   */
  it('parses an indexed port ref `connect src -> vents["intake"].inlet`', () => {
    expect(countErrorNodes('structure def F { connect src -> vents["intake"].inlet }')).toBe(0);
  });

  it('yields ConnectStatement and PortMapping nodes', () => {
    const names = nodeNames('structure def F { connect a -> b { x -> y } }');
    expect(names).toContain('ConnectStatement');
    expect(names).toContain('PortMapping');
  });

  // ── Chain ─────────────────────────────────────────────
  // Corpus-attested: examples/m5_occurrence_process.ri:23.
  it('parses a two-link chain `chain a.p -> b.q`', () => {
    expect(countErrorNodes('structure def F { chain a.p -> b.q }')).toBe(0);
  });

  // Corpus-attested: examples/m5_connect_chain.ri — a four-link chain.
  it('parses a four-link chain', () => {
    expect(
      countErrorNodes('structure def F { chain p1.outlet -> p2.inlet -> p2.outlet -> p3.inlet }'),
    ).toBe(0);
  });

  it('yields a ChainStatement node', () => {
    expect(nodeNames('structure def F { chain a.p -> b.q }')).toContain('ChainStatement');
  });

  // ── Forall statement ──────────────────────────────────
  // grammar.js:1248-1263 — the body must start with `constraint`, `connect` or
  // `chain`; that leading keyword is the ONLY thing separating the statement
  // form from the quantifier EXPRESSION below, so all three bodies are pinned.
  it('parses `forall x in xs: constraint x > 0mm`', () => {
    expect(countErrorNodes('structure def F { forall x in xs: constraint x > 0mm }')).toBe(0);
  });

  it('parses `forall x in xs: connect x.p -> y.q`', () => {
    expect(countErrorNodes('structure def F { forall x in xs: connect x.p -> y.q }')).toBe(0);
  });

  it('parses `forall x in xs: chain x.a -> x.b`', () => {
    expect(countErrorNodes('structure def F { forall x in xs: chain x.a -> x.b }')).toBe(0);
  });

  /**
   * Corpus-attested: examples/m6_forall_index.ri — a range collection, an
   * indexed subscript, and arithmetic inside the subscript, all in the body of
   * one statement. This is the shape that needs the range slice (step-8) and
   * the index-access slice (step-4) to already be in place.
   */
  it('parses the attested `forall i in 0..3 : constraint idlers[i].od < idlers[i + 1].od`', () => {
    expect(
      countErrorNodes(
        'structure def F { forall i in 0..3 : constraint idlers[i].od < idlers[i + 1].od }',
      ),
    ).toBe(0);
  });

  it('yields a ForallStatement node', () => {
    expect(nodeNames('structure def F { forall x in xs: constraint x > 0mm }')).toContain(
      'ForallStatement',
    );
  });

  // ── Quantifier expression ─────────────────────────────
  // grammar.js:1265-1273. Same keyword, EXPRESSION position, and no leading
  // keyword required after the `:`.
  // Corpus-attested: examples/m9_purpose_manufacturability.ri and 4 more.
  it('parses a quantifier inside a constraint `constraint forall m in self.members: determined(m)`', () => {
    expect(
      countErrorNodes('structure def F { constraint forall m in self.members: determined(m) }'),
    ).toBe(0);
  });

  // Corpus-attested: examples/quantifier_undef.ri.
  it('parses a quantifier bound by a let `let all_big = forall v in vs : v.od > 1mm`', () => {
    expect(countErrorNodes('structure def F { let all_big = forall v in vs : v.od > 1mm }')).toBe(0);
  });

  // Corpus-attested: examples/quantifier_undef.ri — the `exists` quantifier.
  it('parses `let any_big = exists x in xs : x >= 3`', () => {
    expect(countErrorNodes('structure def F { let any_big = exists x in xs : x >= 3 }')).toBe(0);
  });

  it('parses `constraint exists x in sizes: x > 10`', () => {
    expect(countErrorNodes('structure def F { constraint exists x in sizes: x > 10 }')).toBe(0);
  });

  it('yields a QuantifierExpression node', () => {
    expect(nodeNames('structure def F { let a = forall v in vs : v.od > 1mm }')).toContain(
      'QuantifierExpression',
    );
  });

  /**
   * REGRESSION GUARD — the `<-`/`<->` versus `<`/`<=` collision. Declaring the
   * two new arrow tokens without an explicit longest-match `@precedence` leaves
   * the arrows unformed; declaring it in the wrong ORDER makes `a <= b` lex as
   * `<` `=` instead. Both readings are pinned, and `leftOperandOf` is used on
   * the comparison so the assertion cannot pass on a differently-grouped tree
   * with the same node multiset.
   */
  it('still parses `constraint a <= b` and `constraint a < b` as comparisons', () => {
    expect(countErrorNodes('structure def F { constraint a <= b }')).toBe(0);
    expect(countErrorNodes('structure def F { constraint a < b }')).toBe(0);
    expect(leftOperandOf('structure def F { constraint a + b <= c }')).toBe('a + b');
  });

  /**
   * REGRESSION GUARD — `direction` and `frame` must stay ORDINARY identifiers
   * outside a port body. `at` already taught this lesson once: a context-free
   * `kw<>` promotion un-pinned two committed files that pass the word as a
   * named-argument label. Both of these words are attested as a `let` name, a
   * `param` name, and a named-argument label respectively, so the same
   * `ekw<>`/`@extend` treatment is mandatory and this pins it.
   */
  it('still parses `direction` and `frame` as ordinary identifiers', () => {
    expect(countErrorNodes('structure def F { let direction = normalize(velocity) }')).toBe(0);
    expect(countErrorNodes('structure def F { param frame : Frame3 = f() }')).toBe(0);
    expect(countErrorNodes('structure def F { let x = g(frame: mid_f) }')).toBe(0);
    expect(countErrorNodes('structure def F { let x = self.frame }')).toBe(0);
  });

  /**
   * REGRESSION GUARD — `in` and `out` leave the ReservedWord list in this
   * slice, so they stop being reachable from `primaryExpression`. Nothing in
   * the corpus uses either as an identifier (verified: no `.in`/`.out` member
   * access, no `in:`/`out:` argument label), but the member forms that DO use
   * them must all still parse, and an ordinary block must be unaffected.
   */
  it('still parses an ordinary structure body after promoting `in`/`out`', () => {
    expect(countErrorNodes('structure def F { param w : Length = 80mm let x = w * 2 }')).toBe(0);
  });
});

/**
 * `enum` declarations, `match` expressions, and the variant
 * construction/binding pair they travel with.
 *
 * THE BRACE COLLISION IS WHAT MAKES THIS SLICE HARD. `variant_construction` is
 * `identifier '{' field: value, … '}'` (grammar.js:1443-1451), so ANY
 * expression that reduces to a bare identifier and is followed by `{` could
 * start one. Upstream declares an explicit GLR conflict
 * `[_primary_expression, variant_construction]` for it. Lezer is LR, so the
 * same collision has to be settled a different way — and there are three
 * committed shapes it must not break, each with its own regression assertion
 * at the bottom of this block:
 *
 *    match outline { … }              discriminant, then `{`
 *    where shape == Shape.Round { … } guard condition, then `{`
 *    connect outlet -> inlet { … }    right port ref, then `{`
 *
 * The `=>` token is shared with `MapEntry` (added for map literals) and `|`
 * with `LambdaExpression`, so both of those get regression assertions too.
 */
describe('reify.grammar snippets — match, enum, variants', () => {
  // ── Enum declarations ─────────────────────────────────
  // Corpus-attested: examples/integration_full_v01.ri:38,
  // examples/m5_guarded_enum.ri:1, examples/m5_combined_all.ri:9.
  it('parses a bare-variant enum `enum Grade { Standard, Reinforced, Premium }`', () => {
    expect(countErrorNodes('enum Grade { Standard, Reinforced, Premium }')).toBe(0);
  });

  it('parses a single-variant enum `enum Shape { Point }`', () => {
    expect(countErrorNodes('enum Shape { Point }')).toBe(0);
  });

  // Corpus-attested: examples/m6_data_carrying_enum.ri:12-16 — payload
  // variants, a bare variant, and a trailing comma in one body.
  it('parses a payload-bearing enum with a trailing comma', () => {
    expect(
      countErrorNodes(
        'enum Shape { Circle { radius: Length }, Rect { width: Length, height: Length }, Point, }',
      ),
    ).toBe(0);
  });

  // Corpus-attested: examples/m6_generic_enum.ri:1-9 — a two-parameter enum
  // and a recursive one-parameter enum whose payload type is itself applied.
  it('parses a parameterized enum `enum Result<T, E> { Ok { value: T }, Err { error: E } }`', () => {
    expect(countErrorNodes('enum Result<T, E> { Ok { value: T }, Err { error: E } }')).toBe(0);
  });

  it('parses a recursive parameterized enum `enum Tree<T> { Node { left: Tree<T> } }`', () => {
    expect(countErrorNodes('enum Tree<T> { Leaf { value: T }, Node { left: Tree<T> } }')).toBe(0);
  });

  // grammar.js:154 makes `pub` optional on the declaration; unattested.
  it('parses `pub enum Grade { Standard }` (normative, unattested)', () => {
    expect(countErrorNodes('pub enum Grade { Standard }')).toBe(0);
  });

  it('yields EnumDeclaration, EnumVariant and VariantFieldDecl nodes', () => {
    const names = nodeNames('enum Shape { Circle { radius: Length }, Point }');
    expect(names).toContain('EnumDeclaration');
    expect(names).toContain('EnumVariant');
    expect(names).toContain('VariantFieldDecl');
  });

  // ── Match expressions ─────────────────────────────────
  // Corpus-attested: examples/integration_full_v01.ri:146-150,
  // examples/m5_guarded_enum.ri:13-17, examples/m5_combined_all.ri:28-31.
  it('parses a bare-pattern match bound by a let', () => {
    expect(
      countErrorNodes(
        'structure def F { let code = match grade { Standard => 1, Reinforced => 2, Premium => 3 } }',
      ),
    ).toBe(0);
  });

  it('parses a trailing comma after the last match arm', () => {
    expect(countErrorNodes('structure def F { let c = match g { A => 1, B => 2, } }')).toBe(0);
  });

  // Corpus-attested: examples/m6_data_carrying_enum.ri:21-25 — binding
  // patterns with one and two fields, and arithmetic in the arm bodies.
  it('parses binding-pattern match arms', () => {
    expect(
      countErrorNodes(
        'structure def F { let area = match outline { Circle { radius: r } => 3.14159 * r * r, Rect { width: w, height: h } => w * h, Point => 0mm * 0mm } }',
      ),
    ).toBe(0);
  });

  /**
   * The wildcard and or-patterns (grammar.js:1289-1293) have no corpus
   * occurrence — every committed match enumerates its variants explicitly.
   * The or-pattern is the reason `|` needs to keep working in TWO roles, so
   * it is pinned here alongside the lambda regression assertion below.
   */
  it('parses a wildcard arm `match s { _ => 0mm }` and yields a WildcardPattern node (normative, unattested)', () => {
    const src = 'structure def F { let a = match s { _ => 0mm } }';
    expect(countErrorNodes(src)).toBe(0);
    // Count, not `toContain`: exactly ONE node spans the `_`. See
    // `countNodesNamed` for the doubling this pins against.
    expect(countNodesNamed(src, 'WildcardPattern')).toBe(1);
  });

  it('parses an or-pattern arm `match s { A | B => 1mm }` (normative, unattested)', () => {
    expect(countErrorNodes('structure def F { let a = match s { A | B => 1mm } }')).toBe(0);
  });

  it('yields MatchExpression, MatchArm and VariantBindingPattern nodes', () => {
    const names = nodeNames('structure def F { let a = match s { Circle { radius: r } => r } }');
    expect(names).toContain('MatchExpression');
    expect(names).toContain('MatchArm');
    expect(names).toContain('VariantBindingPattern');
  });

  // ── Variant construction ──────────────────────────────
  // Corpus-attested: examples/m6_data_carrying_enum.ri:19,
  // examples/m6_data_carrying_enum_undef.ri:19, examples/m6_generic_enum.ri:29.
  it('parses a variant construction `param outline : Shape = Rect { width: 20mm, height: 10mm }`', () => {
    expect(
      countErrorNodes('structure def F { param outline : Shape = Rect { width: 20mm, height: 10mm } }'),
    ).toBe(0);
  });

  /**
   * Corpus-attested: examples/m6_generic_enum.ri:36. The field VALUE of a
   * variant construction is a full `_expression` upstream, so a construction
   * nests inside a construction — the assertion that stops the field value
   * being narrowed to a literal.
   */
  it('parses a nested variant construction', () => {
    expect(
      countErrorNodes(
        'structure def F { param tree : Tree<Length> = Node { left: Leaf { value: 1mm }, right: Leaf { value: 2mm } } }',
      ),
    ).toBe(0);
  });

  it('parses a variant construction as a `let` value', () => {
    expect(countErrorNodes('structure def F { let s = Circle { radius: 5mm } }')).toBe(0);
  });

  it('yields VariantConstruction and VariantConstructionField nodes', () => {
    const names = nodeNames('structure def F { let s = Circle { radius: 5mm } }');
    expect(names).toContain('VariantConstruction');
    expect(names).toContain('VariantConstructionField');
  });

  /**
   * REGRESSION GUARD — the `match … {` discriminant. This is the exact
   * collision grammar.js declares its GLR conflict for: `outline` is a bare
   * identifier and the very next token is `{`, so a parser that lets
   * VariantConstruction win here consumes the whole match body as a variant
   * payload. `nodeNames` is the discriminator, because the greedy misreading
   * can be error-free — it would be wrong, not broken.
   */
  it('reads `match outline { … }` as a match, not a variant construction', () => {
    const names = nodeNames('structure def F { let a = match outline { Point => 1mm } }');
    expect(names).toContain('MatchExpression');
    expect(names).not.toContain('VariantConstruction');
  });

  /**
   * REGRESSION GUARD — the guarded block. Corpus-attested twice
   * (examples/m5_guarded_enum.ri:7, examples/m5_guarded_head_type.ri:7), and
   * the bare-identifier condition, though unattested, is the shape that
   * collides hardest.
   */
  it('still parses a guarded block whose condition ends in an identifier', () => {
    expect(countErrorNodes('structure def F { where x > 1mm { let a = 1mm } }')).toBe(0);
    expect(
      countErrorNodes('structure def F { where shape == Shape.Round { let a = 1mm } }'),
    ).toBe(0);
    expect(countErrorNodes('structure def F { where flag { let a = 1mm } }')).toBe(0);
  });

  /**
   * REGRESSION GUARD — the connect body. Its right port ref is a full
   * expression and is followed by `{`, so it collides with variant
   * construction the same way the match discriminant does.
   */
  it('still parses a connect body after adding variant construction', () => {
    const names = nodeNames(
      'structure def F { connect outlet -> inlet { diameter -> diameter } }',
    );
    expect(names).toContain('ConnectBody');
    expect(names).not.toContain('VariantConstruction');
    expect(countErrorNodes('structure def F { connect a -> b : C { gain = auto } }')).toBe(0);
  });

  /**
   * REGRESSION GUARD — the two tokens this slice shares with earlier ones.
   * `=>` is `MapEntry`'s separator as well as `MatchArm`'s, and `|` is
   * `LambdaExpression`'s delimiter as well as the or-pattern's. Either could
   * be re-ranked by this slice's additions.
   */
  it('still parses map literals and lambdas after adding match arms', () => {
    expect(countErrorNodes('structure def F { let m = map { 1 => 2 } }')).toBe(0);
    expect(countErrorNodes('structure def F { let f = |x| x * 2 }')).toBe(0);
    expect(countErrorNodes('structure def F { let f = || 1 }')).toBe(0);
    expect(countErrorNodes('structure def F { constraint a || b }')).toBe(0);
  });
});

/**
 * ── `_` is a WILDCARD IN PATTERN POSITION AND AN ORDINARY IDENTIFIER
 *    EVERYWHERE ELSE ────────────────────────────────────────────────────────
 *
 * Why `_` is declared the way it is — the `@extend` + `@dynamicPrecedence`
 * mechanism, the compiler evidence that `_` is a bound name rather than a
 * reserved word, and the variants that were tried and rejected — lives in the
 * `WildcardPattern` note in reify.grammar. It is not restated here.
 *
 * What these tests pin is the IDENTIFIER half of the contract, in the four
 * positions where nothing else in this file would notice it breaking: `_` must
 * parse clean AND must not reduce to `WildcardPattern`. Error-freedom alone is
 * too weak — a widening of the precedence that made `_` a wildcard in a `let`
 * or argument position would still parse clean and leave every one of these
 * green while the contract was gone.
 *
 * They are DELIBERATELY separate `it` blocks: a bad declaration regresses all
 * six at once, so naming each keeps the failure output pointing at the form
 * that actually broke. The `WildcardPattern` half of the contract is pinned by
 * the expression form's own wildcard test and the `MatchArmDeclBlock` one;
 * both directions have to hold simultaneously.
 */
describe('reify.grammar snippets — `_` stays a legal ordinary identifier', () => {
  it('accepts `_` as a `let` binding name', () => {
    const src = 'structure def F { let _ = 3mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('Identifier');
    expect(nodeNames(src)).not.toContain('WildcardPattern');
  });

  it('accepts `_` as a `param` name', () => {
    const src = 'structure def F { param _ : Length = 1mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('Identifier');
    expect(nodeNames(src)).not.toContain('WildcardPattern');
  });

  it('accepts `_` as an `fn` parameter name', () => {
    const src = 'structure def H { fn f(_ : Length) -> Length { 1mm } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('Identifier');
    expect(nodeNames(src)).not.toContain('WildcardPattern');
  });

  it('accepts `_` in argument position', () => {
    const src = 'structure def F { let a = f(_) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('Identifier');
    expect(nodeNames(src)).not.toContain('WildcardPattern');
  });

  /**
   * The two or-pattern arms. CONTRACT PINNED: `_` in an OR-SLOT reduces to an
   * ordinary `Identifier`, never a `WildcardPattern` — the faithful reading of
   * the authoritative grammar and lowering, not a gap. #5958 first tried to
   * "fix" it into a `WildcardPattern` and had to be reverted; WHY, with the
   * grammar.js / ts_parser.rs citations and the measured compiler behaviour,
   * is in the `DO NOT "FIX" ...` note on `MatchPattern` in reify.grammar, next
   * to the rule where the tempting change gets typed. Not restated here.
   *
   * MEASURED for THIS form (`reify eval`, this branch):
   *   - `match k { A | _ => 1mm }`
   *       -> `error: non-exhaustive match on 'Kind': missing variant(s) B, C`
   *     `_` did NOT act as a catch-all.
   *   - `match k { _ | A => 1mm }` -> `Parse error: syntax error`.
   *     The compiler rejects this source outright, so the zero-ERROR pin below
   *     is deliberate EDITOR LENIENCY, explicitly not compiler parity.
   *
   * Both halves are asserted: `WildcardPattern` count 0 (what `_` must NOT be)
   * plus the node actually spanning `_` (what it must BE). The count alone
   * would stay green if `_` left the tree entirely or came back under a third
   * name — the same class of hole `countNodesNamed` was added (#5957) to close.
   */
  it('accepts `_` as the trailing element of an or-pattern', () => {
    const src = 'structure def F { let a = match p { A | _ => 1mm } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'WildcardPattern')).toBe(0);
    expect(nodeNamesSpanning(src, '_')).toEqual(['Identifier']);
  });

  it('accepts `_` as the leading element of an or-pattern', () => {
    const src = 'structure def F { let a = match p { _ | A => 1mm } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'WildcardPattern')).toBe(0);
    expect(nodeNamesSpanning(src, '_')).toEqual(['Identifier']);
  });
});

/**
 * The remaining declaration families: `field def`, `purpose`, `unit`, the
 * top-level `type` alias, `default`, the `meta` block, and the `#pragma` /
 * `@annotation` pair.
 *
 * THE KEYWORD-PROMOTION RISK IS THE WHOLE STORY IN THIS SLICE. Seven of the
 * words involved are attested as ORDINARY IDENTIFIERS in committed `.ri` —
 * `source` 37 times (`connect source -> sink` alone accounts for most),
 * `offset` 8, `field:` 8 as an argument label, `unit:` 6 as an argument label,
 * `composed` and `imported` as `let`/`sub` names. Every one of those has to
 * stay a legal name, which is what `ekw<>`/`@extend` is for, and each gets a
 * regression assertion at the bottom.
 *
 * Two of them — `field` and `unit` — are in the ReservedWord @specialize list
 * TODAY, so their label form does not parse today either. Promoting them
 * contextually is a strict gain, not merely a non-regression.
 */
describe('reify.grammar snippets — remaining declaration families', () => {
  // ── Field definitions ─────────────────────────────────
  // Corpus-attested: examples/fea_shell_channels.ri:24-26,
  // examples/fields/fn_field.ri. The analytical source body is a LAMBDA, so
  // this arm depends on the lambda slice already being in place.
  it('parses an analytical field definition', () => {
    expect(
      countErrorNodes('field def top_layer : Real -> Real { source = analytical { |x| 100.0 } }'),
    ).toBe(0);
  });

  // Corpus-attested: examples/differential_field_ops.ri:44-52 — config
  // entries are newline-separated, NOT comma-separated (grammar.js:339-342
  // spells it `repeat($.field_config_entry)`).
  it('parses a sampled field definition with config entries', () => {
    expect(
      countErrorNodes(
        'field def quadratic : Length -> Real { source = sampled { grid = "RegularGrid1" spacing = 1.0m data = [0.0, 1.0, 4.0] } }',
      ),
    ).toBe(0);
  });

  // Corpus-attested: examples/fields/composed_stiffness.ri:18.
  it('parses a composed field definition', () => {
    expect(
      countErrorNodes(
        'field def cs : Real -> Real { source = composed { |p| 2.0 * p + 1.0 } }',
      ),
    ).toBe(0);
  });

  // Corpus-attested: examples/imported_field/openvdb_stress.ri:23.
  it('parses an imported field definition', () => {
    expect(
      countErrorNodes('field def s : Real -> Real { source = imported { format = OpenVDB } }'),
    ).toBe(0);
  });

  it('yields a FieldDefinition node', () => {
    expect(
      nodeNames('field def f : Real -> Real { source = analytical { |x| 1.0 } }'),
    ).toContain('FieldDefinition');
  });

  // ── Purpose declarations ──────────────────────────────
  // Corpus-attested: examples/determinacy_intrinsics.ri:28,
  // examples/integration_full_v01.ri:355.
  it('parses a purpose declaration with one param', () => {
    expect(
      countErrorNodes('purpose design_review(subject : Structure) { constraint 1mm > 0mm }'),
    ).toBe(0);
  });

  // Corpus-attested: examples/ambient_default_material/ambient_default_surface.ri:41
  // — an empty parameter list.
  it('parses a purpose declaration with no params', () => {
    expect(countErrorNodes('purpose Exploration() { constraint 1mm > 0mm }')).toBe(0);
  });

  // Corpus-attested: examples/integration_full_v01.ri:371.
  it('parses `pub purpose weight_target(part : Structure) { … }`', () => {
    expect(countErrorNodes('purpose weight_target(part : Structure) { minimize part.mass }')).toBe(
      0,
    );
    expect(countErrorNodes('pub purpose w(part : Structure) { minimize part.mass }')).toBe(0);
  });

  it('yields a PurposeDeclaration node', () => {
    expect(nodeNames('purpose p(s : Structure) { constraint 1mm > 0mm }')).toContain(
      'PurposeDeclaration',
    );
  });

  // ── Unit declarations ─────────────────────────────────
  // Corpus-attested: examples/integration_full_v01.ri:33, examples/m9_combined.ri:46.
  it('parses a unit declaration `unit mil : Length = 0.0000254`', () => {
    expect(countErrorNodes('unit mil : Length = 0.0000254')).toBe(0);
  });

  it('parses a unit declaration with a quantity conversion', () => {
    expect(countErrorNodes('unit inch : Length = 25.4mm')).toBe(0);
  });

  // grammar.js:432-434 — both the conversion and the offset are optional.
  it('parses the bodyless and offset unit forms (normative, unattested)', () => {
    expect(countErrorNodes('unit furlong : Length')).toBe(0);
    expect(countErrorNodes('unit degF : Temperature = 0.5556 offset 255.372')).toBe(0);
  });

  // ── Type aliases ──────────────────────────────────────
  // Corpus-attested: examples/integration_corner_cases.ri:23-25,
  // examples/integration_full_v01.ri:28.
  it('parses a dimensional type alias `type Pressure = Force / Area`', () => {
    expect(countErrorNodes('type Pressure = Force / Area')).toBe(0);
    expect(countErrorNodes('type Velocity = Length / Time')).toBe(0);
  });

  it('parses a multi-operator and a parameterized type alias', () => {
    expect(countErrorNodes('type Power = Force * Length / Time')).toBe(0);
    expect(countErrorNodes('type Stress<T> = Force / Area')).toBe(0);
  });

  // ── Default declarations ──────────────────────────────
  // Corpus-attested: tests/prd-gate/fixtures/purpose_nested_structure.ri:14
  // (top level) and .../ambient_default_surface.ri:30,42 (nested in a purpose).
  it('parses a default declaration `default Material = steel`', () => {
    expect(countErrorNodes('default Material = steel')).toBe(0);
  });

  it('parses a default declaration whose value is a call', () => {
    expect(
      countErrorNodes('default Material = Material(name: "steel", density: 7850kg/m^3)'),
    ).toBe(0);
  });

  it('parses a default declaration inside a purpose body', () => {
    expect(countErrorNodes('purpose E() { default Material = aluminum }')).toBe(0);
  });

  // ── Meta blocks ───────────────────────────────────────
  // Corpus-attested: examples/integration_full_v01.ri:120-123,
  // examples/m9_combined.ri:54. Comma-separated, unlike the field config
  // entries above.
  it('parses a meta block', () => {
    expect(
      countErrorNodes('structure def F { meta { project = "integration-test", version = "0.1" } }'),
    ).toBe(0);
  });

  // ── Pragmas and annotations ───────────────────────────
  // Corpus-attested: examples/integration_full_v01.ri:22-23,
  // examples/multi_kernel/pragma_override.ri:21,
  // examples/conditional_compilation/main.ri:3.
  it('parses the attested pragma forms', () => {
    expect(countErrorNodes('#version(0.1)')).toBe(0);
    expect(countErrorNodes('#precision(0.001m)')).toBe(0);
    expect(countErrorNodes('#kernel(occt)')).toBe(0);
    expect(countErrorNodes('#cfg(target = "linux")')).toBe(0);
  });

  it('parses a bare pragma with no argument list (normative, unattested)', () => {
    expect(countErrorNodes('#experimental')).toBe(0);
  });

  // Corpus-attested: examples/integration_full_v01.ri:294-318 — an annotation
  // preceding a structure definition.
  it('parses an annotation preceding a declaration', () => {
    expect(countErrorNodes('@test structure TestHeightPositive { constraint 1mm > 0mm }')).toBe(0);
  });

  /**
   * REGRESSION GUARD — every word this slice promotes that is ALSO attested as
   * an ordinary identifier. `source` is the sharpest case: `connect source ->
   * sink` is a committed connect statement, so a context-free promotion would
   * break the topology slice that landed immediately before this one.
   */
  it('keeps `source`, `offset`, `composed` and `imported` as ordinary identifiers', () => {
    expect(countErrorNodes('structure def F { connect source -> sink }')).toBe(0);
    expect(countErrorNodes('structure def F { param offset : Length = 1mm }')).toBe(0);
    expect(countErrorNodes('structure def F { let composed = 1mm }')).toBe(0);
    expect(countErrorNodes('structure def F { sub imported : Part }')).toBe(0);
    expect(countErrorNodes('structure def F { let x = f(source: a, offset: b) }')).toBe(0);
  });

  /**
   * `field` and `unit` are in the ReservedWord @specialize list TODAY, so
   * their attested argument-label form (`field:` 8 times, `unit:` 6) does not
   * parse today. Promoting them CONTEXTUALLY has to fix that, not merely
   * preserve it — which is why this is an assertion and not a comment.
   */
  it('makes `field` and `unit` usable as argument labels', () => {
    expect(countErrorNodes('structure def F { let x = f(field: a, unit: b) }')).toBe(0);
  });

  /**
   * REGRESSION GUARD — the `@` collision. An annotation opens a declaration
   * with `@`; the ad-hoc selector uses `@` INFIX after a complete expression.
   * The two never share a parse state, and this pins that they still do not.
   */
  it('still parses an ad-hoc selector after adding annotations', () => {
    expect(countErrorNodes('structure def F { let top = post @ face("top") }')).toBe(0);
    expect(nodeNames('structure def F { let top = post @ face("top") }')).toContain('AdHocSelector');
  });

  /**
   * REGRESSION GUARD — `type` is already a `kw<>` for the member-level
   * AssociatedType (`type Item = Length` inside a trait). The top-level alias
   * reuses the same keyword with a different RHS grammar, so both readings
   * must survive.
   */
  it('still parses a member-level associated type after adding the top-level alias', () => {
    expect(countErrorNodes('trait T { type Item }')).toBe(0);
    expect(countErrorNodes('structure def F { type Material = Steel }')).toBe(0);
  });
});

/**
 * Radix, imaginary and interpolated-string literals.
 *
 * WHY THESE ARE SHAPE ASSERTIONS AND NOT ERROR COUNTS. All four forms already
 * parse with ZERO error nodes today, so an error-count assertion would be
 * vacuously green and pin nothing:
 *
 *   `0xFF`      the `QuantityLiteral` token reads it as `0` + unit `xFF`
 *   `4.1j`                     ""                     `4.1` + unit `j`
 *   `1_000_000`                ""                     `1` + unit `_000_000`
 *   `"a {b} c"` the `String` token has no brace exclusion, so it swallows the
 *               interpolation holes as ordinary string bytes
 *
 * Every one of those is the WRONG node under the right span. So these tests
 * assert the node SHAPE via `nodeNames`, and the corpus files that are clean
 * *because of* the catch-all get explicit non-regression assertions at the
 * bottom — a narrower purpose-built token that misses a shape would silently
 * un-pin them, which is the one outcome this slice must not produce.
 */
describe('reify.grammar snippets — radix, imaginary and interpolated literals', () => {
  // ── Radix literals ────────────────────────────────────
  // Corpus-attested: examples/radix_literals.ri:11,14,17 and
  // examples/numeric_and_range_literals.ri:19,22.
  it('yields a RadixLiteral for hex `0xFF`', () => {
    const names = nodeNames('structure def F { let mask = 0xFF }');
    expect(names).toContain('RadixLiteral');
    expect(names).not.toContain('QuantityLiteral');
  });

  it('yields a RadixLiteral for binary `0b1010`', () => {
    const names = nodeNames('structure def F { let flags = 0b1010 }');
    expect(names).toContain('RadixLiteral');
    expect(names).not.toContain('QuantityLiteral');
  });

  // examples/radix_literals.ri:17 — `_` digit separators inside the radix run.
  // The upstream scanner consumes them as part of the literal
  // (tree-sitter-reify/src/scanner.c, RADIX_LITERAL digit loop), so the whole
  // `0xDEAD_BEEF` is ONE node, not a literal followed by an identifier.
  it('yields a single RadixLiteral for the separator-bearing `0xDEAD_BEEF`', () => {
    const src = 'structure def F { let addr = 0xDEAD_BEEF }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('RadixLiteral');
  });

  it('accepts uppercase radix prefixes `0XFF` and `0B1010`', () => {
    expect(nodeNames('structure def F { let a = 0XFF }')).toContain('RadixLiteral');
    expect(nodeNames('structure def F { let a = 0B1010 }')).toContain('RadixLiteral');
  });

  /**
   * DIVERGENCE FROM THE STEP TEXT, RECORDED DELIBERATELY. The plan step lists
   * octal `0o17` alongside hex and binary, but reify has NO octal literal:
   * tree-sitter-reify/src/scanner.c, RADIX_LITERAL block, admits exactly
   * `x`/`X` (hex) and `b`/`B` (binary) as radix prefixes and returns false for
   * anything else, which leaves `0o17` to the quantity path as
   * `quantity_literal(0, unit "o17")`. Implementing `0o` would invent language
   * surface the authoritative grammar does not have, so this pins the upstream
   * reading instead.
   */
  it('leaves `0o17` as a QuantityLiteral — reify has no octal radix', () => {
    const src = 'structure def F { let a = 0o17 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('QuantityLiteral');
    expect(nodeNames(src)).not.toContain('RadixLiteral');
  });

  /**
   * The unit-suffixed radix form. grammar.js:1484-1497 gives `quantity_literal`
   * a dedicated RADIX ARM (`0xFFmm` → value `0xFF`, unit `mm`), so a narrower
   * RadixLiteral token that wins here would turn a VALID form into an error
   * node — a capability regression, not a shape fix. `0x`/`0b` with no digits
   * is likewise a quantity (`quantity_literal(0, unit "x")`) per the
   * "Prefix with no digits" branch of the scanner.
   */
  it('keeps the unit-suffixed radix `0xFFmm` a QuantityLiteral', () => {
    const src = 'structure def F { let a = 0xFFmm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('QuantityLiteral');
    expect(nodeNames(src)).not.toContain('RadixLiteral');
  });

  it('keeps the digitless `0x` a QuantityLiteral', () => {
    const src = 'structure def F { let a = 0x }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('QuantityLiteral');
  });

  // ── Imaginary literals ────────────────────────────────
  // Corpus-attested: examples/complex_literals.ri:14,21 (`4.1j`, `4j`),
  // examples/complex_transcendental.ri (`0j`).
  it('yields an ImaginaryLiteral for `4j` and `4.1j`', () => {
    expect(nodeNames('structure def F { let w = 3 + 4j }')).toContain('ImaginaryLiteral');
    expect(nodeNames('structure def F { let z = 3.2 + 4.1j }')).toContain('ImaginaryLiteral');
  });

  // grammar.js:1460 names `1.5e-3j` as a valid imaginary literal, so the
  // numeric part carries an exponent (upstream `number_literal` is
  // /\d(_?\d)*(\.\d(_?\d)*)?([eE][+-]?\d(_?\d)*)?/, grammar.js:1626).
  it('yields an ImaginaryLiteral for the exponent-bearing `1.5e-3j`', () => {
    const src = 'structure def F { let z = 1.5e-3j }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('ImaginaryLiteral');
  });

  /**
   * THE `j`-UNIT CARVE-OUT. grammar.js:1459-1468 documents that the scanner
   * refuses the imaginary reading when a word character follows the `j`, so
   * multi-char j-units (`jk`, `joule`) and capital `J` (joule) stay quantity
   * literals. A token that grabbed `2j` out of `2joule` would leave `oule`
   * dangling as an identifier — an error node on a valid unit.
   */
  it('keeps `2joule` and `5J` quantity literals, not imaginary literals', () => {
    for (const src of ['structure def F { let e = 2joule }', 'structure def F { let e = 5J }']) {
      expect(countErrorNodes(src)).toBe(0);
      expect(nodeNames(src)).toContain('QuantityLiteral');
      expect(nodeNames(src)).not.toContain('ImaginaryLiteral');
    }
  });

  // ── Interpolated strings ──────────────────────────────
  /**
   * SYNTAX CORRECTION, RECORDED DELIBERATELY. The plan step spells the hole
   * `"a ${b} c"`, a JavaScript-ism. Reify's hole is a BARE brace — grammar.js
   * :1645-1670 (`interpolated_string` / `interpolation`) and the committed
   * examples/interpolation.ri:18 (`"thickness is {t}, doubled is {2 * t}"`)
   * both spell it `{expr}`. The corpus is the arbiter, so these assertions use
   * the attested form; `${b}` would be one chunk containing a literal `$`.
   */
  it('yields an InterpolatedString with an Interpolation hole', () => {
    const src = 'structure def F { let s = "a {b} c" }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('InterpolatedString');
    expect(names).toContain('Interpolation');
  });

  // examples/interpolation.ri:18 — two holes, one of them a full expression.
  it('parses a multi-hole interpolation whose hole is a whole expression', () => {
    const src = 'structure def F { let s = "thickness is {t}, doubled is {2 * t}" }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('BinaryExpression');
  });

  /**
   * examples/interpolation.ri:25 — `{{`/`}}` are CONTENT, not holes (scanner.c
   * STRING_CONTENT: "doubled open/close brace → both chars consumed as
   * content"), so there is no `Interpolation` node here.
   *
   * DIVERGENCE, RECORDED. Upstream calls this an `interpolated_string` with one
   * chunk and no holes, because ITS split is on the presence of a BRACE: the
   * whole-string `token()` has a brace-excluding char class and fails here.
   * Lezer cannot keep a whole-string token at all — it overlaps the bare `"`
   * delimiter, and the resulting precedence applies at every quote in the file
   * (see the `String` rule in reify.grammar for the measured failure) — so both
   * readings are built from the same two tokens and the split lands on the
   * presence of a HOLE instead. Both grammars agree on the only semantic claim
   * being made here, which is that the doubled braces are literal content.
   */
  it('treats doubled braces `"{{braces}}"` as content, not a hole', () => {
    const src = 'structure def F { let s = "{{braces}}" }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('String');
    expect(names).not.toContain('Interpolation');
  });

  /**
   * REGRESSION GUARD for the node name every other consumer sees. Admitting
   * interpolation cost the whole-string token, so `String` became a RULE over
   * `stringQuote`/`StringChunk`. The node name and its span must be unchanged,
   * or 300-odd pinned files silently change shape and the `String: t.string`
   * styleTags rule stops matching anything.
   */
  it('still parses a brace-free string as a String node', () => {
    const names = nodeNames('structure def F { let s = "steel" }');
    expect(names).toContain('String');
    expect(names).not.toContain('InterpolatedString');
  });

  it('still parses the empty string and the legacy `import "foo.ri"` form', () => {
    expect(countErrorNodes('structure def F { let s = "" }')).toBe(0);
    expect(nodeNames('import "foo.ri"')).toContain('String');
  });

  /**
   * `StringChunk` admits `{{` and `}}` as units, which forces it ABOVE the
   * `"{"`/`"}"` delimiters in the token precedence order. This pins that the
   * doubled-brace units do not leak OUT of string bodies: `}}` closing two
   * nested blocks at once is ordinary source and must keep parsing clean.
   */
  it('still parses adjacent block-closing braces `}}` in ordinary code', () => {
    expect(countErrorNodes('structure def F { let m = map { 1 => 2 }}')).toBe(0);
  });

  it('still parses a string containing an escape and a comment marker', () => {
    const src = 'structure def F { let s = "a \\" b // c" }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('String');
  });

  /**
   * THE USER-VISIBLE COST of retiring the whole-string token, pinned rather
   * than left to be rediscovered. A LONE unescaped brace inside a string is now
   * an error node: measured 0 errors under the old token and 1 now, for
   * `"use {} for empty"`, `"a { b"` and `"}"` alike.
   *
   * This is upstream-faithful, not a defect of the port. grammar.js:1628-1643
   * narrows `string_literal`'s char class from `/[^"\\]/` to `/[^"\\{}]/`
   * precisely so an unescaped brace falls through to `interpolated_string`, and
   * grammar.js:1664-1666 records that the empty hole `{}` is then "a parse
   * error for free". Upstream also grepped the corpus and found zero string
   * literals with a bare brace, which is why nothing committed regresses.
   */
  it('rejects a lone unescaped brace inside a string (upstream-faithful)', () => {
    expect(countErrorNodes('structure def F { let s = "use {} for empty" }')).toBeGreaterThan(0);
    expect(countErrorNodes('structure def F { let s = "a { b" }')).toBeGreaterThan(0);
  });

  /**
   * The escape hatch a user reaching for a literal brace actually needs, and
   * the more valuable half of the pair: nothing else in this file guards it.
   * `StringChunk`'s `"\\" _` arm mirrors upstream's `seq('\\', /./)`.
   */
  it('accepts escaped braces `"a\\{b\\}"` as ordinary string content', () => {
    const src = 'structure def F { let s = "a\\{b\\}" }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('String');
    expect(names).not.toContain('Interpolation');
  });

  // ── Non-regression on the files that are clean BECAUSE of the catch-all ──
  /**
   * These four are pinned in EXPECTED_CLEAN today, and three of them are clean
   * only because `QuantityLiteral` swallows their literals whole. A narrower
   * token that misses one shape would un-pin them — the exact failure mode this
   * slice is most likely to produce, so it gets a direct assertion rather than
   * relying on the ledger to notice at the end of the run.
   */
  it.each([
    'examples/radix_literals.ri',
    'examples/complex_literals.ri',
    'examples/numeric_separators.ri',
    'examples/interpolation.ri',
  ])('still parses %s with zero error nodes', (path) => {
    expect(countErrorNodes(readFixture(path))).toBe(0);
  });

  /**
   * The `_`-separated DECIMAL literal is deliberately NOT re-shaped here.
   * `1_000_000` still lexes as `QuantityLiteral` (`1` + unit `_000_000`), and
   * `1_000mm` (examples/numeric_separators.ri:17) still lexes as one quantity.
   * Splitting them off would need a separator-aware Number token ordered ABOVE
   * QuantityLiteral, and lezer token precedence beats longest-match — so that
   * token would take `1_000` out of the ATTESTED `1_000mm` and strand the
   * `mm`, turning a committed line into an error node. The exponent form
   * `1.0e6` rides along on the same catch-all (`1.0` + unit `e6`); it is
   * asserted here too, because the divergence note in reify.grammar claims
   * both halves are pinned and that claim should be true.
   */
  it('leaves `_`-separated decimals as quantity literals (documented divergence)', () => {
    const big = 'structure def F { let big = 1_000_000 }';
    expect(countErrorNodes(big)).toBe(0);
    expect(nodeNames(big)).toContain('QuantityLiteral');
    expect(countErrorNodes('structure def F { let len = 1_000mm }')).toBe(0);
  });

  it('leaves exponent-form decimals as quantity literals (same divergence)', () => {
    const src = 'structure def F { let d = 1.0e6 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('QuantityLiteral');
  });
});

/**
 * `relate { … }` member blocks — grammar.js:724-734.
 *
 * The whole ledger movement of this catch-up round: five committed files
 * (examples/geometric_relations/{bolt_plate,construction_datum,global_float}.ri
 * and examples/kinematic/relate_mounted_{fourbar,revolute}.ri) each carry 3-4
 * error nodes, ALL of them inside a relate block, and the block is their only
 * blocker.
 *
 * The body is a bare-expression repeat, so the relations are newline-separated
 * with NO comma — the shape the two-relation case below pins directly.
 */
describe('reify.grammar snippets — relate blocks', () => {
  // Corpus-attested verbatim: examples/geometric_relations/global_float.ri:31-36.
  it('parses a single-relation relate block', () => {
    const src = 'structure def F { relate { fasten(a.frame, b.frame) } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('RelateBlock');
    expect(names).toContain('RelationMember');
  });

  /**
   * Corpus-attested verbatim: examples/kinematic/relate_mounted_revolute.ri:77-80.
   * Two relations on separate lines with NO separator between them — the half
   * a single-relation case cannot cover, since a comma-separated body would
   * pass that one and fail this.
   */
  it('parses a two-relation relate block with no separator', () => {
    const src =
      'structure def F {\n  relate {\n    concentric(link.hub_axis, base.mount_axis)\n    flush(link.hub_plane, base.mount_plane)\n  }\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('RelateBlock');
  });

  // Upstream's `repeat` is zero-or-more, so an empty body is legal.
  it('parses an empty relate block', () => {
    const src = 'structure def F { relate { } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('RelateBlock');
  });

  /**
   * Three of the five corpus blocks carry `//` comments on or above their
   * relation lines, and one of them (global_float.ri:32-34) opens with two
   * comment lines before the first relation. A comment-only body is the
   * degenerate case of that and must still be a well-formed empty block.
   */
  it('parses a comment-only relate block', () => {
    const src = 'structure def F {\n  relate {\n    // grounds neither operand\n  }\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('RelateBlock');
  });

  /**
   * THE TOKEN-GROUP CONSEQUENCE, pinned where it was caused. `RelationMember*`
   * is the grammar's first repeat of BARE expressions, which is what merged the
   * `"|"` and `"||"` token groups — see the note on LambdaExpression in
   * reify.grammar and the two `||` assertions above.
   *
   * Inside a relate body the merge leaves a genuine choice at `||`: continue
   * the current relation as a binary expression, or end it and open a new
   * relation with a zero-parameter lambda. Greedy shift wins (the `!or` level
   * is tighter than `!lambda`), so a relate body containing `a || b` is ONE
   * relation. Both readings are error-free, so this is asserted on node names.
   */
  it('reads `||` inside a relate body as one binary relation', () => {
    const src = 'structure def F { relate { a || b } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('BinaryExpression');
    expect(names).not.toContain('LambdaExpression');
  });

  it('styles `relate` as a keyword in the block slot', () => {
    expect(keywordSpans('structure def F { relate { fasten(a, b) } }')).toContain('relate');
  });

  /**
   * THE `ekw` CONTRACT, mirroring the `self` receiver test below. `relate` is
   * spelled `ekw<"relate">` (contextual) rather than `kw<"relate">`, matching
   * upstream's treatment at grammar.js:714-719, so it must still lex as an
   * ordinary identifier off the block slot — and must NOT be styled there.
   * Both halves are asserted: a `@specialize` would keep the parse green while
   * silently painting an ordinary variable named `relate` as a keyword.
   */
  it('leaves `relate` an ordinary identifier off the block slot', () => {
    const src = 'structure def F { let relate = 1mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(keywordSpans(src)).not.toContain('relate');
  });
});

/**
 * `constraint def` — grammar.js:400-420, a top-level DECLARATION (not a member).
 *
 * Six committed files use it; only examples/m9_constraint_def.ri and
 * examples/m9_integration.ri turn clean on this family alone. The other four
 * each keep a second, out-of-scope blocker (`some(u)`, `meta.material`,
 * `meta.project`, a member-level `@annotation`), so their error counts drop but
 * they stay off EXPECTED_CLEAN — see the MEASUREMENT block above it.
 *
 * THE SHAPE THAT MAKES THIS FAMILY HARD is a body whose repeat mixes items
 * carrying an OPTIONAL TAIL (`ParamDeclaration`, `LetDeclaration`, `Pragma`)
 * with a BARE-EXPRESSION `ConstraintDefPredicate`, so the tail's opening token
 * is also a token the next predicate could open with. The three conflicts that
 * produces, the three shifts marked to settle them, and the trade greedy shift
 * accepts are in the `itemStart` note in reify.grammar's @precedence block.
 *
 * WHAT THIS SLICE ADDS is both sides of that trade, pinned: five preserved
 * readings and, at the bottom, the one sacrificed. The preserved five are
 * asserted on node NAMES rather than error counts, because a precedence change
 * reshapes a tree silently and both readings parse clean; the sacrificed one is
 * the only place an error count is the right instrument, since there the losing
 * reading does not parse at all.
 */
describe('reify.grammar snippets — constraint def', () => {
  // Corpus-attested verbatim: examples/m9_constraint_def.ri:18-21.
  it('parses a single-predicate constraint def', () => {
    const src = 'constraint def MinThickness {\n    param t: Length\n    t > 1mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ConstraintDefinition');
    expect(names).toContain('ConstraintDefPredicate');
  });

  /**
   * Corpus-attested verbatim: examples/m9_constraint_def.ri:25-31. The case a
   * single-predicate body cannot cover — CONSECUTIVE bare predicates with no
   * separator between them, and params preceding them in the same repeat.
   */
  it('parses a multi-param, multi-predicate constraint def with no separators', () => {
    const src =
      'constraint def Bounded {\n' +
      '    param x: Length\n' +
      '    param lo: Length\n' +
      '    param hi: Length\n' +
      '    x >= lo\n' +
      '    x <= hi\n' +
      '}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ConstraintDefinition');
    expect(names).toContain('ConstraintDefPredicate');
  });

  /**
   * Corpus-attested verbatim: examples/integration_corner_cases.ri:57-59. The
   * predicate repeat is zero-or-more, so a body of params alone is legal —
   * the case that would break if the body were written `params* predicates+`.
   */
  it('parses the vacuous form with zero predicates', () => {
    const src = 'constraint def Vacuous {\n    param v : Length\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ConstraintDefinition');
    expect(names).not.toContain('ConstraintDefPredicate');
  });

  // Corpus-attested verbatim: examples/m9_constraint_def.ri:44-47.
  it('parses a `pub constraint def` header', () => {
    const src = 'pub constraint def Positive {\n    param v: Length\n    v > 0mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('ConstraintDefinition');
  });

  /**
   * Normative, unattested. Upstream admits `let` in the body alongside `param`
   * (grammar.js:411-416); no committed file uses it, but the item is in the
   * same repeat as the predicates, so it shares their conflict shape and is
   * pinned rather than dropped.
   */
  it('parses a `let` body item beside a predicate (normative, unattested)', () => {
    const src = 'constraint def Derived {\n    param t: Length\n    let m = t * 2\n    m > 1mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('LetDeclaration');
    expect(names).toContain('ConstraintDefPredicate');
  });

  // Normative, unattested — the header reuses the same TypeParameters
  // production the structure/trait/enum headers do, so it comes free.
  it('parses type parameters on the header (normative, unattested)', () => {
    const src = 'constraint def C<T> {\n    param v: T\n    v > 0mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ConstraintDefinition');
    expect(names).toContain('TypeParameters');
  });

  /**
   * THE DISCRIMINATION PIN. Member-level `constraint <expr>` and top-level
   * `constraint def <Name>` share their first token, and the wrong reading of
   * either would be error-free. Position settles it — `Member` is unreachable
   * from the top level and `Declaration` from inside a body — but that is a
   * property of the grammar's shape, not of a marker, so it is asserted.
   */
  it('still reads member-level `constraint` as a ConstraintDeclaration', () => {
    const src = 'structure def F { param t : Length = 2mm  constraint t > 1mm }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ConstraintDeclaration');
    expect(names).not.toContain('ConstraintDefinition');
  });

  /**
   * ── The `itemStart` regression pins ───────────────────────────────────────
   *
   * Each of the five below is a reading greedy shift PRESERVES, asserted
   * INSIDE a constraint-def body because that is the state where the conflict
   * actually lives — every one of them is attested at declaration or member
   * level elsewhere in the corpus. The sixth pin, last in the slice, is the
   * mirror image these cost.
   */
  it('keeps `Box<T>` a ParameterizedType inside a constraint-def body', () => {
    const src = 'constraint def C {\n    param b : Box<T>\n    b > 1mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('ParameterizedType');
  });

  it('keeps the multi-arg `Tensor<2, 3, Force>` a ParameterizedType there too', () => {
    const src = 'constraint def C {\n    param t : Tensor<2, 3, Force>\n    t > 1mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('ParameterizedType');
  });

  it('keeps `auto(free)` and `auto(seed = 5mm)` AutoKeywords there', () => {
    const free = 'constraint def C {\n    param x : Length = auto(free)\n    x > 1mm\n}';
    expect(countErrorNodes(free)).toBe(0);
    expect(nodeNames(free)).toContain('AutoKeyword');

    const seeded = 'constraint def C {\n    param x : Length = auto(seed = 5mm)\n    x > 1mm\n}';
    expect(countErrorNodes(seeded)).toBe(0);
    const names = nodeNames(seeded);
    expect(names).toContain('AutoKeyword');
    expect(names).toContain('AutoParam');
  });

  it('keeps a parenthesised pragma arg list attached to its Pragma', () => {
    const src = 'constraint def C {\n    #pragma_name(a = 1)\n    x > 1mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('Pragma');
    expect(names).toContain('PragmaArg');
  });

  /**
   * The other half of the pragma pin, and the one greedy shift could quietly
   * swallow: a BARE `#pragma_name` whose following item does NOT open with
   * `(`. The pragma must end at its identifier and the predicate must be its
   * own node — two items, not one pragma with an argument list.
   */
  it('ends a bare pragma at its identifier when the next item is a predicate', () => {
    const src = 'constraint def C {\n    #pragma_name\n    x > 1mm\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('Pragma');
    expect(names).toContain('ConstraintDefPredicate');
    expect(names).not.toContain('PragmaArg');
  });

  /**
   * THE SACRIFICE ITSELF, pinned rather than left to the prose — the mirror of
   * the still-narrowed `primaryExpression` pin in the VariantConstruction slice.
   * The five assertions above say what greedy shift KEEPS; without this one
   * nothing says what it COSTS, so a later precedence edit could swap which side
   * loses and the suite would stay green.
   *
   * MEASURED, both at 1 error node: after an `auto`-defaulted param a following
   * `(x > 1mm)` is swallowed as the `AutoParamList` (so it never becomes a
   * predicate at all), and after a plain `Name` type a following `<10mm` is
   * swallowed as the opening `<` of a `ParameterizedType`. Neither shape occurs
   * in any of the 330 committed `.ri`, which is what makes the trade payable.
   */
  it('does not admit a predicate opening with `(` or `<` after an item with an optional tail', () => {
    const afterAuto = 'constraint def C {\n    param x : Length = auto\n    (x > 1mm)\n}';
    expect(countErrorNodes(afterAuto)).toBeGreaterThan(0);
    expect(nodeNames(afterAuto)).not.toContain('ConstraintDefPredicate');

    const afterType = 'constraint def C {\n    param b : Name\n    <10mm\n}';
    expect(countErrorNodes(afterType)).toBeGreaterThan(0);
  });
});

/**
 * Argument-position `VariantConstruction` — a WIDENING of the port's one
 * deliberately narrowed production, and a family that moves the ledger by zero
 * files. It is taken for node SHAPE, so every assertion here is what guards it.
 *
 * THE CLAIM UNDER TEST is that argument position is structurally different from
 * `primaryExpression` — a difference of FOLLOW SET, not of marker. Why that
 * holds, and which three committed shapes the surviving narrowing protects, are
 * on `VariantConstruction` in reify.grammar. This slice tests both halves: the
 * widening works, and the narrowing it does not touch is still in force.
 *
 * EVERY WIDENING AND COLLISION ASSERTION BELOW IS ON NODE NAMES, never on an
 * error count, because in each collision shape the WRONG reading is error-free
 * — a construction would silently swallow the match/guard/connect body, and a
 * count is blind to that. The one exception is the final pin, where the
 * still-rejected shape genuinely errors.
 */
describe('reify.grammar snippets — variant construction in argument position', () => {
  /**
   * The shape the grammar note has recorded as rejected since the narrowing
   * was introduced — MEASURED then at 3 error nodes. Normative: no committed
   * `.ri` uses it, which is why this family cannot move EXPECTED_CLEAN.
   */
  it('parses a construction as a positional argument (normative, unattested)', () => {
    const src = 'structure def F { let x = f(Circle { radius: 5mm }) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('VariantConstruction');
    expect(names).toContain('VariantConstructionField');
  });

  /**
   * The named-argument slot, and the reason widening only the positional arm
   * would leave the gap half-closed: `NamedArgument` is the value slot of the
   * same call syntax, and is SHARED with `SubDeclaration`'s instantiation
   * `ArgList` — so this is the form a user is most likely to write.
   */
  it('parses a construction as a named argument on a sub instantiation', () => {
    const src = 'structure def F { sub s = Foo(shape: Circle { radius: 5mm }) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('VariantConstruction');
    expect(names).toContain('NamedArgument');
  });

  // A construction whose own field value is another construction, reached
  // through an argument list — the two widened slots composing.
  it('parses a nested construction inside an argument', () => {
    const src = 'structure def F { let x = f(Outer { inner: Circle { radius: 5mm } }) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('VariantConstruction');
  });

  // Positional and named arguments mix freely (grammar.js:1542-1546), so a
  // construction must survive beside both without disturbing the list.
  it('parses a construction in a mixed positional/named argument list', () => {
    const src = 'structure def F { let x = f(1mm, Circle { radius: 5mm }, other: 2mm) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('VariantConstruction');
    expect(names).toContain('NamedArgument');
  });

  /**
   * ── The three collision pins ──────────────────────────────────────────────
   *
   * The shapes the narrowing exists to protect. Each is committed and
   * load-bearing, and in each the construction reading would parse CLEAN while
   * swallowing the body — so these are the assertions that would catch an
   * over-eager future widening into `primaryExpression`.
   */
  it('still reads `match d { … }` as a MatchExpression, not a construction', () => {
    const src = 'structure def F { let x = match outline { Round => 1mm, Square => 2mm } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('MatchExpression');
    expect(names).toContain('MatchArm');
    expect(names).not.toContain('VariantConstruction');
  });

  it('still reads `where cond { … }` as a GuardedBlock, not a construction', () => {
    const src = 'structure def F { where shape == Shape.Round { param d : Length = 1mm } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('GuardedBlock');
    expect(names).not.toContain('VariantConstruction');
  });

  it('still reads `connect a -> b { … }` as a ConnectBody, not a construction', () => {
    const src = 'structure def F { connect outlet -> inlet { a -> b } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ConnectBody');
    expect(names).toContain('PortMapping');
    expect(names).not.toContain('VariantConstruction');
  });

  /**
   * THE FOURTH PIN, and the one that keeps the narrowing honest. Widening the
   * two argument slots must NOT quietly widen `primaryExpression` as well: a
   * bare `Name { … }` in ordinary expression position still has `{` in its
   * follow set, nothing measured has changed about that, and it must still be
   * an error. Without this, a later reader cannot tell the deliberate
   * narrowing from an accidental one.
   */
  it('still rejects a bare construction in ordinary expression position', () => {
    const src = 'structure def F { constraint Circle { radius: 5mm } == x }';
    expect(countErrorNodes(src)).toBeGreaterThan(0);
  });
});

/**
 * `sub_relate_block` — grammar.js:745-750, the sub-local sibling of `relate`:
 * a conditionless `where { … }` trailing a sub's pose, holding relations
 * scoped to that sub rather than to the enclosing structure.
 *
 * Zero committed files use it, so this family moves the ledger by nothing and
 * is taken for node shape — but it carries the round's only real REGRESSION
 * risk, and the pins below are the whole point of the slice.
 *
 * A post-pose `where` is genuinely ambiguous to LR(1): it may open a
 * SubRelateBlock or end the SubDeclaration so that `where` starts a member-level
 * GuardedBlock. Why that is resolved with a `~poseWhere` ambiguity marker rather
 * than with the `itemStart` precedence level one family earlier is on
 * `PoseClause` in reify.grammar.
 *
 * WHAT THIS SLICE MUST SHOW is that BOTH readings survive, because both are
 * committed: poses at examples/geometric_relations/construction_datum.ri:62 and
 * global_float.ri:28 (`sub bolt : Bolt at auto`), and member-level
 * `where determined(origin) { … }` GuardedBlocks at
 * examples/integration_full_v01.ri:224 and examples/m10_combined.ri:97, in six
 * committed files overall. Every assertion here is on node NAMES: both parses
 * are error-free, so a count cannot tell them apart.
 */
describe('reify.grammar snippets — sub relate blocks', () => {
  // Normative, unattested. The specialization-form sub, whose pose is `at auto`
  // exactly as construction_datum.ri:62 and global_float.ri:28 write it.
  it('parses a conditionless `where { … }` after a pose (normative, unattested)', () => {
    const src = 'structure def F { sub s : T at auto where { concentric(a, b) } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('PoseClause');
    expect(names).toContain('SubRelateBlock');
    expect(names).toContain('RelationMember');
  });

  // The instantiation-form sub reaches `PoseClause` through the other arm, so
  // the block must attach there too.
  it('parses a sub relate block on an instantiation-form sub', () => {
    const src = 'structure def F { sub s = T() at transform3(x, y, z) where { fasten(a, b) } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('PoseClause');
    expect(names).toContain('SubRelateBlock');
  });

  /**
   * The body reuses `RelationMember` — the same production `RelateBlock` uses,
   * exactly as upstream does at grammar.js:748 — so relations are
   * newline-separated with NO comma here as well.
   */
  it('parses a multi-relation sub relate block with no separator', () => {
    const src =
      'structure def F {\n  sub s : T at auto where {\n    concentric(s.axis, base.axis)\n    flush(s.face, base.face)\n  }\n}';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('SubRelateBlock');
  });

  // The repeat is zero-or-more, as in `RelateBlock`.
  it('parses an empty sub relate block', () => {
    const src = 'structure def F { sub s : T at auto where { } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('SubRelateBlock');
  });

  /**
   * ── THE REGRESSION PIN ────────────────────────────────────────────────────
   *
   * A member-level guarded block immediately after a pose. This is the shape
   * greedy shift would steal, and it must stay a GuardedBlock. The guard
   * expression is `determined(origin)` verbatim from
   * examples/integration_full_v01.ri:224 and examples/m10_combined.ri:97.
   */
  it('still reads a conditioned post-pose `where` as a GuardedBlock', () => {
    const src =
      'structure def F {\n  sub s : T at auto\n  where determined(origin) { param d : Length = 1mm }\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('GuardedBlock');
    expect(names).not.toContain('SubRelateBlock');
  });

  // `GuardedBlock` carries an optional `else` arm; a SubRelateBlock has none,
  // so this is the half of the pin that would fail loudest if shift won.
  it('still reads a post-pose `where … { … } else { … }` as a GuardedBlock', () => {
    const src =
      'structure def F {\n  sub s : T at auto\n  where determined(origin) { param d : Length = 1mm } else { param d : Length = 2mm }\n}';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('GuardedBlock');
    expect(names).not.toContain('SubRelateBlock');
  });

  /**
   * The THIRD role `where` plays on a sub, and the one that sits positionally
   * BEFORE the pose: a guard on the declaration itself
   * (`sub child = RecursiveBeam(…) where depth > 0`,
   * examples/integration_full_v01.ri:108). All three must stay distinguishable.
   */
  it('still reads a pre-pose `where <expr>` guard as a WhereClause', () => {
    const src = 'structure def F { sub s : T where enabled at auto }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('WhereClause');
    expect(names).toContain('PoseClause');
    expect(names).not.toContain('SubRelateBlock');
  });
});

/**
 * `match_arm_decl_block` — grammar.js:1322-1345. A MEMBER-position `match`
 * whose arms declare a sub rather than evaluate to a value: the variant chosen
 * decides which sub the structure gets.
 *
 * Zero committed files use it, so it moves the ledger by nothing and is taken
 * for node shape; the assertions below are what guard it.
 *
 * IT NEVER COMPETES WITH `MatchExpression`, and no marker says so — position
 * does. No `Member` can begin with a bare expression, so the expression form is
 * simply unreachable here and the two never share a state. That is the same
 * positional discriminator `ForallStatement` vs `QuantifierExpression` already
 * relies on. The two pins below assert it in both directions.
 */
describe('reify.grammar snippets — match arm declaration blocks', () => {
  // Normative, unattested — as is every case in this slice.
  it('parses a two-arm declaration block (normative, unattested)', () => {
    const src = 'structure def F { match kind { Hex => sub head : HexHead, Sq => sub head : SqHead } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('MatchArmDeclBlock');
    expect(names).toContain('MatchArmDeclArm');
    expect(names).toContain('MatchArmSubDecl');
  });

  /**
   * The arms reuse the existing `MatchPattern` production verbatim, so the
   * or-pattern, the wildcard and the variant-binding form come free and cannot
   * drift from the ones `MatchExpression` accepts. Each is asserted rather than
   * assumed, since "comes free" is a claim about the grammar's shape — the
   * wildcard case below used to show why that was worth checking (it came free
   * including a pre-existing divergence, fixed by #5957) and stays as a direct
   * assertion now that the divergence is gone.
   */
  it('accepts an or-pattern arm', () => {
    const src = 'structure def F { match kind { Hex | Button => sub head : RecessedHead } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('MatchArmDeclBlock');
  });

  /**
   * The or-pattern arm carrying `_`, in both slot positions. `match_arm_decl_arm`
   * reuses `$.match_pattern` verbatim (grammar.js:1330-1334), exactly as the
   * plain or-pattern case above does, so this form inherits the SAME contract —
   * `_` in an or-slot is an ordinary `Identifier`, not a wildcard. Why that is
   * the faithful reading, and why #5958's attempt to change it was reverted, is
   * in the `DO NOT "FIX" ...` note on `MatchPattern` in reify.grammar; not
   * restated here.
   *
   * MEASURED for THIS form (`reify eval`, this branch) — the diagnostics differ
   * from the expression form's, which is why they are recorded separately:
   *   - `match kind { Hex | _ => sub head : DefaultHead }`
   *       -> `error: match-arm pattern '_' is not a variant of enum 'Kind'`
   *   - `match kind { _ | Hex => sub head : DefaultHead }`
   *       -> `Parse error: syntax error`
   *
   * As in the expression form, the `_ | Hex` zero-ERROR pin is deliberate
   * EDITOR LENIENCY, not compiler parity, and both halves of the node contract
   * are asserted — count 0 for what `_` must not be, plus the node spanning `_`
   * for what it must be.
   */
  it('accepts an or-pattern arm carrying `_`, which stays an Identifier', () => {
    const trailing = 'structure def F { match kind { Hex | _ => sub head : DefaultHead } }';
    expect(countErrorNodes(trailing)).toBe(0);
    expect(countNodesNamed(trailing, 'WildcardPattern')).toBe(0);
    expect(nodeNamesSpanning(trailing, '_')).toEqual(['Identifier']);

    const leading = 'structure def F { match kind { _ | Hex => sub head : DefaultHead } }';
    expect(countErrorNodes(leading)).toBe(0);
    expect(countNodesNamed(leading, 'WildcardPattern')).toBe(0);
    expect(nodeNamesSpanning(leading, '_')).toEqual(['Identifier']);
  });

  /**
   * The declaration form must produce the SAME wildcard tree the expression
   * form does — same node, same count — since both go through `MatchPattern`.
   * #5957 fixed a divergence here where neither produced the node at all; how
   * that was diagnosed and fixed is in the `WildcardPattern` note in
   * reify.grammar.
   */
  it('accepts a wildcard arm and yields a WildcardPattern node', () => {
    const src = 'structure def F { match kind { _ => sub head : DefaultHead } }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('MatchArmDeclBlock');
    expect(countNodesNamed(src, 'WildcardPattern')).toBe(1);
  });

  it('accepts a variant-binding-pattern arm', () => {
    const src = 'structure def F { match kind { Hex { size: s } => sub head : HexHead } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('MatchArmDeclBlock');
    expect(names).toContain('VariantBindingPattern');
  });

  it('accepts a single-arm block and a trailing comma', () => {
    const single = 'structure def F { match kind { Hex => sub head : HexHead } }';
    expect(countErrorNodes(single)).toBe(0);
    expect(nodeNames(single)).toContain('MatchArmDeclBlock');

    const trailing = 'structure def F { match kind { Hex => sub head : HexHead, } }';
    expect(countErrorNodes(trailing)).toBe(0);
    expect(nodeNames(trailing)).toContain('MatchArmDeclBlock');
  });

  /**
   * ── The two discrimination pins ───────────────────────────────────────────
   *
   * Expression position is unaffected, and member position admits ONLY the
   * declaration form. Asserted on node names in the first direction because
   * the wrong reading there would be error-free.
   */
  it('leaves expression-position `match` a MatchExpression', () => {
    const src = 'structure def F { let x = match outline { Round => 1mm } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('MatchExpression');
    expect(names).toContain('MatchArm');
    expect(names).not.toContain('MatchArmDeclBlock');
  });

  it('rejects a member-position `match` whose arms are expressions', () => {
    const src = 'structure def F { match k { A => 1mm } }';
    expect(countErrorNodes(src)).toBeGreaterThan(0);
  });

  /**
   * THE RESTRICTION PIN. The arm body is kept at upstream's narrow
   * `sub name : StructName` — no body, no where clause — because the compiler
   * rejects the wider form (audit M-006, entity.rs:2506-2521). This port's
   * standing stance is that over-permissiveness is the safe direction of error,
   * but that only holds where the cost is "no squiggle on a program the
   * compiler will still reject"; here accepting it would invent a third
   * dialect, so the restriction is deliberate and pinned.
   */
  it('rejects a body on a match-arm sub declaration (audit M-006)', () => {
    const src =
      'structure def F { match kind { Hex => sub head : HexHead { param d : Length = 1mm } } }';
    expect(countErrorNodes(src)).toBeGreaterThan(0);
  });
});

/**
 * `chain` STAYS A LEGAL ORDINARY IDENTIFIER — catch-up round 4, family 1.
 *
 * WHY `ekw<"chain">` AND NOT `kw<"chain">`, the round-2 regression it corrects
 * and the measurement behind it all live on the `ChainStatement` production in
 * reify.grammar. Read it there.
 *
 * What these assertions uniquely carry is their FORM, because the demotion's
 * risk is one-sided: every gain it buys shows up as ledger movement, but the
 * thing it could BREAK — the statement reading — would not. So that half is
 * pinned on the node NAME, not an error count. The `ListLiteral` assertion is
 * the other deliberate one: it pins that the list literal the round-3 ledger
 * blamed was never the blocker, so that framing cannot be re-inherited.
 */
describe('reify.grammar snippets — `chain` stays a legal ordinary identifier', () => {
  // Corpus-attested VERBATIM: examples/tolerance-stackup-3part.ri:45
  // (`let chain = [c_bore, c_shaft, c_space, c_ring]`) and the same shape at
  // examples/tolerance-stackup-rss.ri:35 (`let chain = [c1, c2, c3]`).
  it('accepts `chain` as a let-binding name', () => {
    const src = 'structure def S { let chain = [c_bore, c_shaft, c_space, c_ring] }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('LetDeclaration');
    // The list literal was NEVER the blocker — it already parsed under the
    // round-3 grammar, error nodes and all. Pinned so the mis-diagnosis the
    // round-3 ledger recorded cannot be inherited a second time.
    expect(names).toContain('ListLiteral');
  });

  // Corpus-attested: examples/tolerance-stackup-rss.ri:40,44 —
  // `let worst = stackup_worst_case(chain)` / `let rss = stackup_rss(chain)`.
  // The identifier has to reach EXPRESSION position too, not just the binding
  // name, so both are pinned in one snippet.
  it('accepts `chain` as an ordinary expression operand', () => {
    const src = 'structure def S { let chain = a  let worst = f(chain) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('LetDeclaration');
    expect(names).toContain('FunctionCall');
  });

  /**
   * THE READING THAT MUST NOT BE LOST, asserted on the NODE NAME rather than an
   * error count. A demotion that broke the statement reading would still parse
   * `chain x -> y -> z` in some positions — as a bare expression with `->`
   * lambdas or a connect-ish tail — and an error-count assertion would sail
   * straight past it. `ChainStatement` is the contract.
   */
  it('still yields a ChainStatement for the statement form', () => {
    const src = 'structure def S { connect a.p -> b.q  chain x -> y -> z }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('ChainStatement');
    expect(names).toContain('ConnectStatement');
    // Exactly one — a contextual keyword that re-opened an ambiguity could
    // yield a doubled or nested node while the error count stays 0, the
    // `WildcardPattern > WildcardPattern` failure #5957 hit.
    expect(countNodesNamed(src, 'ChainStatement')).toBe(1);
  });
});

/**
 * A `ReservedWord` in CALL position — catch-up round 4, family 2.
 *
 * WHY THE CALLEE AND NOT THE RESERVATION, and the measurement that localised
 * the gap to the callee slot, live on the `FunctionCall` production in
 * reify.grammar. Read it there.
 *
 * What these assertions uniquely carry: the two candidate fixes are
 * INDISTINGUISHABLE by error count. Un-reserving `some`/`none` — dropping them
 * from the `@specialize` list — parses all three corpus files exactly as well,
 * so every `countErrorNodes(...) === 0` assertion in this block passes under
 * either fix. Only the styling pin below, asserted on the node NAME, tells them
 * apart; that is the reason it exists.
 */
describe('reify.grammar snippets — a ReservedWord in call position', () => {
  // Corpus-attested VERBATIM: examples/integration_corner_cases.ri:99.
  it('parses `some(u)`', () => {
    expect(countErrorNodes('structure def S { let s = some(u) }')).toBe(0);
  });

  // Corpus-attested VERBATIM: examples/option_map_or.ri:30 — also exercises a
  // reserved-word call nested as the first argument of an ordinary call, and a
  // lambda argument alongside it.
  it('parses a reserved-word call nested in an argument list, beside a lambda', () => {
    const src = 'structure def S { let m = map_or(some(5mm), 0mm, |x: Length| x * 2.0) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('LambdaExpression');
  });

  // Corpus-attested: tests/prd-gate/fixtures/dcr_yield_stress_dimension_silent.ri:31
  // (`Steel_AISI_1045(yield_stress: some(310mm))`) — a reserved-word call nested
  // inside a NamedArgument VALUE, a third distinct position.
  it('parses a reserved-word call as a named-argument value', () => {
    const src = 'structure def S { let w = Steel(yield_stress: some(310mm)) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('NamedArgument');
  });

  /**
   * THE STYLING PIN — what the fix must NOT cost. The cheap alternative was to
   * drop `some`/`none` from the `ReservedWord` @specialize list, which parses
   * `some(u)` just as well but silently demotes the word to an ordinary
   * `Identifier` and loses its `t.keyword` colour. Asserting the node NAME is
   * the only thing that tells the two fixes apart: both are error-free.
   */
  it('keeps `some` a ReservedWord node when it is the callee', () => {
    const names = nodeNames('structure def S { let s = some(u) }');
    expect(names).toContain('ReservedWord');
    expect(names).toContain('FunctionCall');
  });

  /**
   * NON-REGRESSION. Bare reserved words in expression position already parse
   * via `primaryExpression`; widening the callee must leave that untouched.
   */
  it('still parses a bare reserved word in expression position', () => {
    for (const word of ['none', 'undef']) {
      const src = `structure def S { let s = ${word} }`;
      expect(countErrorNodes(src)).toBe(0);
      expect(nodeNames(src)).toContain('ReservedWord');
    }
  });
});

/**
 * The `%` (modulo) operator — catch-up round 4, family 3.
 *
 * grammar.js:1363-1365 gives `*`, `/` and `%` the SAME `prec.left(6)`, so `%`
 * joins the existing `multiplicative` band rather than getting one of its own.
 * That is the whole change; the interesting part is proving the band is right.
 */
describe('reify.grammar snippets — the `%` operator', () => {
  // Corpus-attested VERBATIM: examples/modulo_index_wrap.ri:13 and :17.
  it('parses `raw_index % count`', () => {
    expect(countErrorNodes('structure def S { let w = raw_index % count }')).toBe(0);
  });

  it('parses `raw_index % 2`', () => {
    expect(countErrorNodes('structure def S { let p = raw_index % 2 }')).toBe(0);
  });

  /**
   * THE PRECEDENCE PIN — the load-bearing assertion in this slice, because both
   * groupings of `a % b * c` parse with an IDENTICAL node multiset. Neither an
   * error count nor a node-name check can tell them apart; only the left
   * operand of the outermost operator node can.
   *
   * If a future edit gives `%` its own node type instead of reusing `ArithOp`,
   * that type MUST be added to `OPERATOR_NODES` in the same change — its
   * docstring records the measured failure mode, where a missing type made
   * `leftOperandOf` walk past the real root and return a confidently wrong
   * answer rather than throw.
   */
  it('binds `%` in the same band as `*` (left-associative)', () => {
    expect(leftOperandOf('structure def F { let x = a % b * c }')).toBe('a % b');
  });

  it('binds `%` tighter than `+`', () => {
    expect(leftOperandOf('structure def F { let x = a + b % c }')).toBe('a');
  });
});

/**
 * `priv` on a param declaration — catch-up round 4, family 4.
 *
 * grammar.js:615-621 opens `param_declaration` with `optional('priv')`. This
 * port already carried `priv` on `SubDeclaration` and `PortDeclaration`, so
 * the keyword and its KEYWORDS entry both exist; only the third member arm was
 * missing.
 */
describe('reify.grammar snippets — `priv` on a param declaration', () => {
  // Corpus-attested VERBATIM: examples/module_visibility/producer.ri:4.
  it('parses `priv param rated_torque : Real = 5`', () => {
    const src = 'structure def S { priv param rated_torque : Real = 5 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('ParamDeclaration');
  });

  /**
   * THE SHAPE PIN. `priv` must be ABSORBED into the declaration, not left as a
   * stray sibling member alongside it — a reading that could parse error-free
   * (a bare `priv` reaching `primaryExpression`) while producing two nodes
   * where the compiler expects one.
   *
   * The `ParamDeclaration` count alone does NOT discriminate that: under the
   * stray-sibling reading there is still exactly one `ParamDeclaration` (it
   * simply starts at `param`), so a count-only pin passes on the broken tree —
   * the #5957 failure class again. The SPAN is what separates them, so it is
   * asserted directly; the `Member` count rules out the extra sibling the
   * broken reading would need somewhere to put.
   */
  it('yields exactly one ParamDeclaration, with `priv` absorbed into it', () => {
    const src = 'structure def S { priv param rated_torque : Real = 5 }';
    expect(countNodesNamed(src, 'ParamDeclaration')).toBe(1);
    expect(countNodesNamed(src, 'Member')).toBe(1);
    expect(sourceOfNodeNamed(src, 'ParamDeclaration')).toBe(
      'priv param rated_torque : Real = 5',
    );
  });

  /**
   * NON-REGRESSION. `Member` now has THREE arms that can open with `priv`
   * (param, sub, port). All three must still parse, since they now share a
   * parse state through that token.
   */
  it('leaves the plain and the other two `priv` member forms intact', () => {
    const plain = 'structure def S { param t : Length = 2mm }';
    expect(countErrorNodes(plain)).toBe(0);
    expect(countNodesNamed(plain, 'ParamDeclaration')).toBe(1);

    expect(countErrorNodes('structure def S { priv sub a = B() }')).toBe(0);
    expect(countErrorNodes('structure def S { priv port p : in Flow }')).toBe(0);
  });
});

/**
 * `auto` as a NAMED-ARGUMENT value — catch-up round 4, family 5.
 *
 * grammar.js:1042-1046 makes `named_argument`'s value field `$._binding_value`,
 * which this port already transliterates as `bindingValue`
 * (`AutoKeyword | VariantConstruction | expression`). `NamedArgument` was
 * spelling its own narrower `(VariantConstruction | expression)`, which reaches
 * every arm EXCEPT `AutoKeyword`.
 */
describe('reify.grammar snippets — `auto` as a named-argument value', () => {
  // Corpus-attested VERBATIM: examples/auto_binding_sites.ri:94.
  it('parses `sub bolt = Bolt(length: auto)`', () => {
    const src = 'structure def S { sub bolt = Bolt(length: auto) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('NamedArgument');
    expect(names).toContain('AutoKeyword');
  });

  /**
   * The parameterised arms come WITH it — `bindingValue` reaches all three
   * `AutoKeyword` forms, not just the bare one, so both are pinned rather than
   * assumed.
   */
  it('parses the parameterised `auto` arms in the same slot', () => {
    for (const value of ['auto(free)', 'auto(seed = 5mm)']) {
      const src = `structure def S { sub b = B(gap: ${value}) }`;
      expect(countErrorNodes(src)).toBe(0);
      expect(nodeNames(src)).toContain('AutoKeyword');
    }
  });

  /**
   * `NamedArgument` is SHARED between call-expression argument lists and
   * `SubDeclaration`'s instantiation `ArgList` — the existing note on the
   * production records that the two must move together. Pinned in the call
   * position too, so a future narrowing cannot split them.
   */
  it('parses `auto` as a call-expression named argument', () => {
    expect(countErrorNodes('structure def S { let x = f(length: auto) }')).toBe(0);
  });

  /**
   * NON-REGRESSION for the arm `bindingValue` already carried. Widening the
   * slot must not disturb the `VariantConstruction` reading, which is the only
   * reason the narrower spelling existed.
   */
  it('still parses a VariantConstruction named-argument value', () => {
    const src = 'structure def S { sub b = B(shape: Circle { radius: 5mm }) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('VariantConstruction');
  });
});

/**
 * An `Annotation` as a MEMBER — catch-up round 4, family 6.
 *
 * grammar.js:44 lists `$.annotation` in `commonMembers()`, so upstream admits
 * an annotation in every body that admits a member — structure, occurrence,
 * guarded and specialization alike. This port only reached `Annotation` from
 * the TOP-LEVEL `Declaration` alternation, so an annotated member inside a
 * structure body had nowhere to go.
 *
 * MEASURED before the fix: examples/m11_annotations.ri errors at L115, L116,
 * L118 and L119, but `structure def S { param b: Length = auto }` on its own
 * parses with 0 errors — so L116/118/119 are cascade from the two annotation
 * lines, and this one change is what the file's gain is attributable to. Test
 * (d) pins that attribution.
 */
describe('reify.grammar snippets — an Annotation as a member', () => {
  // Corpus-attested VERBATIM: examples/m11_annotations.ri:115-116.
  it('parses an annotated param member', () => {
    const src =
      'structure def S { @solver_hint("discrete_set", standard_bolt_lengths)  param bolt_length: Length = auto }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('Annotation');
    expect(names).toContain('ParamDeclaration');
  });

  /**
   * The arg list is optional upstream, so the bare form must come with it —
   * asserted rather than assumed, since `Annotation`'s `(` is the only thing
   * that distinguishes the two arms.
   */
  it('parses a bare annotation member with no argument list', () => {
    const src = 'structure def S { @tag  param t : Length }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('Annotation');
  });

  /**
   * THE DISCRIMINATION PIN, and the load-bearing assertion of this family: the
   * WRONG reading would also parse clean. `Annotation`'s `@` is the SAME token
   * the infix `AdHocSelector` uses, and no precedence separates them. The
   * production's own note argues they "never share a parse state" because an
   * annotation's `@` is only ever at a declaration START and the selector's
   * only ever after a COMPLETE expression. Admitting `Annotation` at MEMBER
   * start makes that claim load-bearing in a SECOND place, so it is asserted
   * here rather than restated: the selector must still reduce to `AdHocSelector`
   * — not to an `Annotation` sitting beside a truncated expression — and its
   * left operand must still be the whole receiver.
   */
  it('does not steal the infix ad-hoc selector', () => {
    const src = 'structure def S { let f = body @ faces_by_normal(zdir) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('AdHocSelector');
    expect(names).not.toContain('Annotation');
    expect(leftOperandOf(src)).toBe('body');
  });

  /**
   * CASCADE ATTRIBUTION. The member an annotation precedes in
   * examples/m11_annotations.ri already parses standalone TODAY, so the file's
   * gain is attributable to the annotation arm alone and not to a second,
   * unnoticed change riding along in the same commit. Passes on arrival.
   */
  it('already parses the annotated member on its own', () => {
    const src = 'structure def S { param b: Length = auto }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'ParamDeclaration')).toBe(1);
  });
});

/**
 * `meta.<field>` member access — catch-up round 4, family 7.
 *
 * `meta` opens `MetaBlock` as a HARD `kw<"meta">`, and a hard keyword in Lezer
 * is context-FREE: the word stops being an `Identifier` everywhere, including
 * as the receiver of an ordinary member access. Upstream has no such problem —
 * tree-sitter's parse-state-driven lexer keeps `meta` an identifier wherever a
 * meta block cannot start — so `ekw<"meta">` is the upstream-faithful reading,
 * exactly as `relate`, `at`, `self`, `default` and (round 4, family 1) `chain`
 * are already spelled here.
 *
 * MEASURED before the fix:
 *   (a) the corpus-verbatim `meta.<field>` lets — 4 errors → RED
 *   (b) the `MetaBlock` reading                 — 0 errors → passes
 *   (c) `meta` as an ordinary binding name      — 2 errors → RED
 *
 * (b) is the trade this demotion is priced against, and it is VERIFIED rather
 * than assumed: it is the reading a careless `ekw` swap would silently cost.
 */
describe('reify.grammar snippets — `meta` stays a legal ordinary identifier', () => {
  // Corpus-attested VERBATIM: examples/m9_combined.ri:67-68
  // (`let label = meta.material` / `let rev   = meta.revision`); the same shape
  // sits at examples/integration_full_v01.ri:140-141 (`meta.project`,
  // `meta.version`).
  it('accepts `meta` as the receiver of a member access', () => {
    const src = 'structure def S { let label = meta.material  let rev = meta.revision }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('LetDeclaration');
    expect(names).toContain('MemberAccess');
  });

  /**
   * THE READING THAT MUST NOT BE LOST — the trade the demotion is priced
   * against, asserted on NODE NAMES and a COUNT rather than an error count
   * alone. A demotion that re-opened an ambiguity at member start could still
   * reach 0 errors while producing a doubled or nested `MetaBlock` — the
   * `WildcardPattern > WildcardPattern` failure #5957 hit, and the same trap
   * family 1's `ChainStatement` pin guards.
   */
  it('still yields exactly one MetaBlock for the block form', () => {
    const src = 'structure def S { meta { material = "steel", revision = "r2" } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('MetaBlock');
    expect(names).toContain('MetaEntry');
    expect(countNodesNamed(src, 'MetaBlock')).toBe(1);
  });

  /**
   * The third position a demotion opens up: `meta` as an ordinary BINDING name,
   * not just as an expression operand. Nothing in the committed corpus needs it
   * today, but it is what "ordinary identifier" means, and it is the position
   * that would silently regress if a later round re-hardened the keyword to buy
   * a disambiguation.
   */
  it('accepts `meta` as an ordinary binding name', () => {
    const src = 'structure def S { let meta = 5mm }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('LetDeclaration');
  });

  /**
   * CASCADE ATTRIBUTION for the SECOND file this family lands.
   * examples/integration_full_v01.ri has a `some(100.0)` at L183 that family 2
   * (a `ReservedWord` in call position) already fixed, so the file turns clean
   * only once BOTH have landed. Pinned here so the ledger's +2 is attributable
   * and a future bisect cannot mis-credit it to the `meta` demotion alone.
   */
  it('already parses the other blocker in integration_full_v01.ri', () => {
    const src = 'structure def S { let maybe_load = some(100.0) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('FunctionCall');
  });
});

/**
 * `::` in EXPRESSION position — catch-up round 4, family 8.
 *
 * `::` already reaches TYPE position through `QualifiedType`
 * (`param p : Beam::Material`). It has never reached EXPRESSION position, so
 * upstream's three productions at grammar.js:1587-1610 have no counterpart
 * here: `qualified_access` (`Foo::bar`, prec.left 9),
 * `instance_qualified_access` (`obj.(Foo::bar)`, prec.left 9) and
 * `trait_method_call` (either of those plus a call tail, prec 11).
 *
 * MEASURED before the fix:
 *   (a) the static  form, corpus-verbatim  — 6 errors  → RED
 *   (b) the instance form, corpus-verbatim — 11 errors → RED
 *   (c) the type-position separation pin    — 0 errors → passes
 *   (d) the plain member-access pin         — 0 errors → passes
 *   (e) the bare qualified access, no tail  — 2 errors → RED
 *
 * (c) and (d) are the load-bearing pins. Only POSITION separates the new
 * expression-position `::` from the existing type-position one, and only the
 * following `(` separates `obj.(Trait::m)` from a plain `obj.m` — both are
 * properties of the grammar's SHAPE rather than of a precedence marker, so
 * neither can be read off the productions and both must be asserted. They pass
 * on arrival and are pinned so the widening cannot quietly capture them.
 */
describe('reify.grammar snippets — `::` in expression position', () => {
  // Corpus-attested VERBATIM: examples/trait_assoc_fn_static.ri:38,42.
  it('parses the static trait-method call form', () => {
    const src =
      'structure def S { let gap : Length = Defaultable::make_default()  let wide : Length = Defaultable::scaled(3.0) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('QualifiedAccess');
    expect(names).toContain('TraitMethodCall');
  });

  // Corpus-attested VERBATIM: examples/trait_assoc_fn_overload.ri:38,62; the
  // same shape at examples/trait_assoc_fn_cylinder.ri:60
  // (`let wetted = pin.(Cylindrical::lateral_area)()`).
  it('parses the trait-disambiguated receiver form', () => {
    const src =
      'structure def S { let a = w.(Measurable::f)(5mm)  let spin = c.(Spinning::rate)() }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('InstanceQualifiedAccess');
    expect(names).toContain('TraitMethodCall');
  });

  /**
   * THE TYPE/EXPRESSION SEPARATION PIN. `QualifiedType` and the new
   * `QualifiedAccess` share the `::` token and differ only in the position they
   * are reached from. Nothing in either production says so, so a widening that
   * let the expression-position reading leak into a type annotation would still
   * parse clean and only show up as the WRONG node. Both the bare base and the
   * applied base (`Coupling<Prismatic>::MotionValue`) are pinned, since the
   * latter reaches `::` through `ParameterizedType` and is the arm most likely
   * to be captured.
   */
  it('does not steal the type-position `::`', () => {
    for (const src of [
      'structure def S { param p : Beam::Material }',
      'structure def S { param p : Coupling<Prismatic>::MotionValue }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
      const names = nodeNames(src);
      expect(names).toContain('QualifiedType');
      expect(names).not.toContain('QualifiedAccess');
    }
  });

  /**
   * THE MEMBER-ACCESS DISCRIMINATION PIN. `InstanceQualifiedAccess` opens
   * `expression "." "("`, which shares its whole prefix with `MemberAccess`'s
   * `expression "." Identifier`. Only the token after the `.` separates them,
   * so a plain dotted chain must still reduce to `MemberAccess` alone.
   */
  it('does not steal a plain member access', () => {
    const src = 'structure def S { let m = self.b.bore }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('MemberAccess');
    expect(names).not.toContain('InstanceQualifiedAccess');
  });

  /**
   * Upstream's `qualified_access` stands alone at prec.left(9) — the call tail
   * belongs to `trait_method_call` at prec(11), a SEPARATE production. So the
   * bare form with no tail has to be admitted on its own; folding the tail into
   * `QualifiedAccess` would parse the whole corpus just as well while diverging
   * from upstream's node shape.
   */
  it('admits a bare qualified access with no call tail', () => {
    const src = 'structure def S { let g = Defaultable::make_default }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('QualifiedAccess');
    expect(names).not.toContain('TraitMethodCall');
  });
});

/**
 * `keyed_member_block` — catch-up round 4, family 9.
 *
 * `sub name : Keyed<T> { "key" => { overrides } }` (grammar.js:890, 906-939).
 * `SubDeclaration`'s `:` arm admits only a `SpecializationBody` here, so the
 * string key had nowhere to go. MEASURED before the fix: the corpus-verbatim
 * block is 10 errors, and the tree shows the failure mode — the `{` opens a
 * `SpecializationBody`, the first key lexes as an `InterpolatedString` and the
 * rest of the block is swept into a `PoseClause`.
 *
 * UPSTREAM'S OWN DISAMBIGUATOR is what makes the widening safe, and (c) is the
 * pin for it: `keyed_member_block` uses `repeat1` while `specialization_body`
 * uses `repeat`, so an empty `{}` is unambiguously a specialization body; when
 * non-empty, the first token after `{` decides — a string selects keyed, an
 * identifier or member keyword the specialization body. That argument is a
 * property of the two productions' SHAPE, so a widening that broke it would
 * still parse clean and differ only in the node produced.
 */
describe('reify.grammar snippets — keyed sub-member blocks', () => {
  // Corpus-attested VERBATIM: examples/keyed_vents.ri:27-30, and the same shape
  // at examples/keyed_forall.ri:18-21.
  it('parses a keyed member block as a sub body', () => {
    const src =
      'structure def S { sub vents : Keyed<Vent> { "intake" => { area = 5mm }  "exhaust" => { area = 8mm } } }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('KeyedMemberBlock');
    expect(names).toContain('KeyedMemberEntry');
    // Exactly two — one per key. A block that collapsed both entries into one,
    // or nested them, would still reach 0 errors (#5957).
    expect(countNodesNamed(src, 'KeyedMemberEntry')).toBe(2);
  });

  /**
   * The overrides are THE SAME NODE a standalone specialization body produces —
   * upstream's stated contract at grammar.js:931-934, "so override blocks
   * inside a keyed sub parse identically to a standalone specialization-scope
   * body". A cloned-but-separate override production would satisfy the error
   * count above and silently fork the two, so the reuse is asserted directly.
   *
   * Asserted on a COUNT, not on membership. MEASURED: the broken tree ALSO
   * contains a `SpecializationBody` and a `ParamAssignment` — it opens one
   * outer body and recovers a param assignment inside it — so a `toContain`
   * pair passes on arrival and pins nothing. The count is 2 (one per keyed
   * entry, with the outer block now a `KeyedMemberBlock`) against 1 today.
   */
  it('reuses SpecializationBody for the overrides', () => {
    const src =
      'structure def S { sub vents : Keyed<Vent> { "intake" => { area = 5mm }  "exhaust" => { area = 8mm } } }';
    expect(countNodesNamed(src, 'SpecializationBody')).toBe(2);
    expect(countNodesNamed(src, 'ParamAssignment')).toBe(2);
  });

  /**
   * UPSTREAM'S DISAMBIGUATOR, pinned. Both block forms open with `{`, so only
   * `repeat1`-vs-`repeat` and the first token after the brace part them. Both
   * readings are attested in the corpus, so both are asserted: the empty body
   * and the identifier-led body must STAY `SpecializationBody`. Passes on
   * arrival; it is the reading the widening could quietly capture.
   */
  it('does not steal the specialization body', () => {
    for (const src of [
      'structure def S { sub x : Foo<T> { } }',
      'structure def S { sub x : Foo<T> { bore = 5mm } }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
      const names = nodeNames(src);
      expect(names).toContain('SpecializationBody');
      expect(names).not.toContain('KeyedMemberBlock');
    }
  });

  /**
   * CASCADE ATTRIBUTION. Everything else the two keyed_* files contain already
   * parses standalone TODAY — the `forall` loop, the key-addressed index chain
   * and the empty port body that follows the block. So the files' whole gain is
   * attributable to the keyed block alone, and their remaining error lines are
   * cascade from it rather than a second gap riding along.
   */
  it('already parses the rest of the keyed files', () => {
    for (const src of [
      'structure def S { forall v in vents: constraint v.area > 1mm }',
      'structure def S { let a = vents["intake"].area }',
      'structure def S { port src : out Flow {} }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
    }
  });
});

/**
 * `joint_definition` — catch-up round 4, family 10.
 *
 * A top-level declaration upstream (grammar.js:147 lists `joint_definition` in
 * `_declaration`; the family itself is :762-802). Nothing of it exists here.
 *
 * BOTH DOF FORMS ARE REQUIRED, which is why (a) and (b) are separate tests
 * rather than one: examples/joint_dof_self_check.ri uses the bare form at L26
 * and the braced-record form at L32, so either alone leaves the file
 * not-clean and the ledger unmoved.
 *
 * MEASURED before the fix:
 *   (a) the bare-DOF form with a range clause — 21 errors → RED
 *   (b) the braced-record form, expr body     — 12 errors → RED
 *   (c) a DOF field with no range clause      — 15 errors → RED
 *   (d) the `pub` / type-parameter headers    — 9, 14   → RED
 *   (e) the contextual-keyword pins           —  0 errors → passes
 */
describe('reify.grammar snippets — joint definitions', () => {
  // Corpus-attested VERBATIM: examples/joint_dof_self_check.ri:26.
  it('parses the bare-DOF form with a range clause', () => {
    const src =
      'joint revolute(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) with angle: Angle in 0deg..120deg = { concentric(a, b)  on(p, stop) }';
    expect(countErrorNodes(src)).toBe(0);
    const names = nodeNames(src);
    expect(names).toContain('JointDefinition');
    expect(names).toContain('JointDofField');
    expect(names).toContain('JointBody');
    // `RelationMember` is δ's node, reused rather than cloned — upstream says
    // so explicitly (grammar.js:795-797), "so each body expression lowers via
    // lower_relation_members (same as relate_block)".
    expect(names).toContain('RelationMember');
  });

  // Corpus-attested VERBATIM: examples/joint_dof_self_check.ri:32. Exercises
  // BOTH the braced-record DOF and the single-EXPRESSION body arm, the two
  // halves the bare form above does not reach.
  it('parses the braced-record DOF form with an expression body', () => {
    const src =
      'joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } = concentric(a, b)';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('JointDefinition');
    // Two fields, not one collapsed node (#5957).
    expect(countNodesNamed(src, 'JointDofField')).toBe(2);
  });

  // The `in <range>` clause is OPTIONAL upstream (grammar.js:791), so the
  // range-less field must be admitted on its own.
  it('parses a DOF field with no range clause', () => {
    const src = 'joint prismatic(a: Axis, b: Axis) with travel: Length = concentric(a, b)';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('JointDofField');
  });

  /**
   * NORMATIVE BUT UNATTESTED. Upstream's header carries `optional('pub')` and
   * `optional($.type_parameters)`, neither of which any committed file uses.
   * They come free from reusing the same `TypeParameters` the structure, trait
   * and fn headers already share — which is exactly why they are pinned: a
   * later round that clones a narrower header instead of reusing it would pass
   * every corpus assertion and silently drop them.
   */
  it('parses the pub and type-parameter headers', () => {
    for (const src of [
      'pub joint j(a: Axis) with x: Length = f(a)',
      'joint j<T>(a: T) with x: Length = f(a)',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
      expect(nodeNames(src)).toContain('JointDefinition');
    }
  });

  /**
   * THE CONTEXTUAL-KEYWORD PINS. `joint` and `with` are contextual upstream,
   * which states the contract directly (grammar.js:757-762): "zero regression:
   * `joint` and `with` continue to lex as identifiers at all other parse
   * positions". A hard `kw<>` would buy the same parses and break exactly this
   * — the failure family 1 (`chain`) and family 7 (`meta`) each had to undo.
   * Passes on arrival, and pins the `ekw<>` spelling as load-bearing.
   */
  it('keeps `joint` and `with` legal ordinary identifiers', () => {
    for (const src of [
      'structure def S { let joint = 5mm }',
      'structure def S { let with = 5mm }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
      expect(nodeNames(src)).toContain('LetDeclaration');
    }
  });

  /**
   * THE PLACEMENT PIN — must STILL ERROR. `joint_definition` is listed in
   * `_declaration` (grammar.js:147) and nowhere in `commonMembers()`, so it is
   * top-level only. Every other assertion in this family would pass just as
   * well with `JointDefinition` also reachable from `Member`, and no ledger
   * movement would reveal the difference — so a later round reaching for
   * `Member` to fix some unrelated body would silently admit what neither
   * tree-sitter nor the compiler does. Same third-dialect divergence the
   * top-level-`param` pin guards on the other side. MEASURED: 2 error nodes.
   */
  it('does not admit a joint definition as a structure member', () => {
    expect(
      countErrorNodes('structure def S { joint j(a: Axis) with x: Length = f(a) }'),
    ).toBeGreaterThan(0);
  });
});

/**
 * `·` (U+00B7) as unit multiplication — catch-up round 4, family 11.
 *
 * WHY `·` CANNOT ESCAPE `unitOp` — the structural safety argument, the PRD §9
 * row B7 and task #5784 boundary cites, and κ's `lexer->lookahead == 0xB7`
 * contract — lives on the `unitOp` token rule in reify.grammar. Read it there.
 *
 * MEASURED before the fix:
 *   (a) `5N·m`                    — 2 errors → RED
 *   (b) `5N·m/rad`                — 3 errors → RED
 *   (c) `5m^2·kg·s^-2`            — 5 errors → RED
 *   (d) the two boundary pins     — 2 errors each → stay RED
 *   (e) the separator divergence  — 0 errors → passes
 *
 * (d) IS THE POINT OF THE FAMILY, not an afterthought: those two snippets must
 * STAY errors after the widening, and unlike every other assertion here, no
 * ledger movement would ever reveal it if they did not.
 */
describe('reify.grammar snippets — `·` as unit multiplication', () => {
  // Corpus-attested VERBATIM: the `torque_like` binding of
  // tests/prd-gate/fixtures/unit_middot_mul.ri. Cited by BINDING NAME, not by
  // line: task #5784 refreshed that fixture's header and shifted every line
  // number, and verify.sh's _RUST_COUPLED_RI_FIXTURES comment records why the
  // repo avoids file:line citations — nothing validates them and they rot on
  // the first edit.
  it('parses a middot-composed unit', () => {
    const src = 'structure def S { let torque_like = 5N·m }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNames(src)).toContain('QuantityLiteral');
  });

  /**
   * The `with_div` binding. Asserted on a COUNT: the `/rad` is part of the UNIT, not a division,
   * so the wrong tree — one quantity divided by an identifier — would also
   * reach 0 errors and an error count alone would sail past it.
   */
  it('keeps a trailing `/unit` inside the same quantity', () => {
    const src = 'structure def S { let with_div = 5N·m/rad }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'QuantityLiteral')).toBe(1);
  });

  // The `composed` binding — `·` composed with `^` and a negative exponent, in
  // one literal. Cited by BINDING NAME for the same reason as its two siblings
  // above: the fixture's line numbers moved under #5784 and nothing validates a
  // file:line cite.
  it('composes middots with unit exponents', () => {
    const src = 'structure def S { let composed = 5m^2·kg·s^-2 }';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'QuantityLiteral')).toBe(1);
  });

  /**
   * THE BOUNDARY PINS, transcribed from the PRD's §9 row B7 and task #5784's
   * measured K2 table. There is no general `·` operator in Reify, and the
   * `·` must not be consumed as a mul-op when no unit atom follows it. Both
   * are errors today and must STAY errors — the only assertions in this family
   * that a ledger gain could not confirm.
   */
  it('does not admit `·` outside a quantity literal', () => {
    for (const src of [
      'structure def S { let x = 5 · 3 }',
      'structure def S { let x = 5N· m }',
    ]) {
      expect(countErrorNodes(src)).toBeGreaterThan(0);
    }
  });

  /**
   * THE STANDING DIVERGENCE, non-regression. Underscore-separated decimals lex
   * as `QuantityLiteral` (`1_000_000` = `1` with unit `_000_000`; `1_000mm` is
   * one quantity). This family touches `unitOp` INSIDE `QuantityLiteral`, so
   * the existing separator pins are its regression net and are restated here at
   * the point of risk.
   */
  it('leaves the underscore-separator reading untouched', () => {
    for (const src of [
      'structure def S { let n = 1_000mm }',
      'structure def S { let n = 1_000_000 }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
      expect(countNodesNamed(src, 'QuantityLiteral')).toBe(1);
    }
  });
});

/**
 * THE TWO FAMILIES ROUND 4 SET OUT TO FIX AND THEN MEASURED AS NON-GAPS.
 *
 * The round-3 note below forecast twelve families from each not-clean file's
 * FIRST blocker. Two of those twelve turned out not to exist as described. This
 * block asserts their absence so it stays visible: nothing else in this file
 * would fail if someone "fixed" either one, and both fixes would be actively
 * harmful.
 *
 * Every assertion here PASSED ON ARRIVAL — that is the point. They are
 * regression pins for a measurement, not RED tests waiting on a change.
 */
describe('reify.grammar — measured non-gaps, pinned so they stay measured', () => {
  /**
   * FAMILY 10, "arrow type in a param annotation". Already reachable. Measured:
   * `structure def S { param f : (Length) -> Length }` → 0 errors, and the
   * parse contains a `FunctionType` node. `FunctionType` has been in the
   * grammar all along; nothing about the ARROW was ever the blocker.
   */
  it('admits an arrow type wherever the language admits a param', () => {
    for (const src of [
      'structure def S { param f : (Length) -> Length }',
      'structure def S { param g : (Length, Angle) -> Real }',
      'structure def S { param h : () -> Length }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
      expect(nodeNames(src)).toContain('FunctionType');
    }
  });

  /**
   * THE COMPLEMENTARY PIN, and the one that actually matters — it is what keeps
   * tests/prd-gate/fixtures/arrow_type.ri off EXPECTED_CLEAN.
   *
   * That fixture is a single TOP-LEVEL `param f : (Length) -> Length`, and its
   * own header (:1-10) declares it a deliberate FAIL probe. The blocker is not
   * the arrow type; it is that `param` IS NOT A TOP-LEVEL DECLARATION. Measured
   * here: the arrow form errors (5 nodes), and so do the plain `param f :
   * Length` (4) and `let x = 1` (4) — none of the three is about arrows.
   *
   * DO NOT "FIX" THIS BY ADMITTING TOP-LEVEL `param`. Two independent sources
   * agree that it is not part of the language, and both were read, not
   * assumed:
   *
   *   - tree-sitter-reify/grammar.js:134 — `_declaration` lists sixteen
   *     alternatives and `param_declaration` is not among them.
   *   - crates/reify-syntax/src/ts_parser.rs:299 — `lower_source_file` has no
   *     `param_declaration` arm; an unexpected named child hits the catch-all
   *     and emits a diagnostic.
   *
   * Admitting it HERE would make the GUI accept what neither the authoritative
   * grammar nor the compiler does — a third dialect, which is exactly what the
   * `AutoTypeArg` note in reify.grammar already forbids. The file stays off the
   * ledger deliberately; see the MEASUREMENT block below.
   */
  it('still refuses a top-level param, which is why arrow_type.ri is not clean', () => {
    for (const src of [
      'param f : (Length) -> Length',
      'param f : Length',
      'let x = 1',
    ]) {
      expect(countErrorNodes(src)).toBeGreaterThan(0);
    }
  });

  /**
   * FAMILY 12's third one-off, "chained comparison `a < b < c < d`". Does not
   * exist either. Measured: 0 errors in both binding and constraint position.
   *
   * examples/integration_corner_cases.ri:131 is `let chain = a < b < c < d`,
   * and every error on that line came from `chain` — which round 2 had promoted
   * to a hard `kw<>` — not from the comparison. Lines 133-135 were cascade.
   * Round 4's family 1 (`chain` back to `ekw<>`) is the whole fix.
   *
   * The `leftOperandOf` assertion pins the GROUPING, not just the absence of
   * errors: a precedence change could re-associate the chain to the right and
   * leave the error count at 0, so the clean parse alone would not notice.
   */
  it('already parses a chained comparison, left-associatively', () => {
    for (const src of [
      'structure def S { let x = a < b < c < d }',
      'structure def S { constraint a < b < c < d }',
    ]) {
      expect(countErrorNodes(src)).toBe(0);
    }
    expect(leftOperandOf('structure def F { let x = a < b < c }')).toBe('a < b');
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

  it('styles `trait`, `fn` and `type` as keywords', () => {
    expect(keywordSpans('trait Seal { }')).toContain('trait');
    expect(keywordSpans('fn area(w: Real) -> Real = w')).toContain('fn');
    expect(keywordSpans('trait T { type Item }')).toContain('type');
  });

  /**
   * `self` is `ekw` (contextual), so it is styled in the RECEIVER slot and
   * left as an ordinary identifier in expression position — the reading
   * tree-sitter gives it too. Both halves are asserted: styling the receiver
   * is the point of the KEYWORDS entry, and NOT styling the expression
   * occurrence is the observable half of the contextual choice.
   */
  it('styles the `self` receiver but not expression-position `self`', () => {
    expect(keywordSpans('trait T { fn f(self) -> Real = 1.0 }')).toContain('self');
    expect(keywordSpans('structure def F { constraint self.b.bore == 10mm }')).not.toContain('self');
  });

  /**
   * CONTROL. `trait` used to be this test's subject; it was promoted out of
   * the ReservedWord list in the same change that added TraitDeclaration, so
   * it stopped being a control and the assertion silently became a second
   * copy of the promoted-keyword check above. The word here must be one that
   * is still in the `ReservedWord` @specialize list in reify.grammar.
   */
  it('still styles a word that remains in the ReservedWord list', () => {
    expect(keywordSpans('structure def F { let x = undef }')).toContain('undef');
  });
});

/**
 * Folding and indentation are configured in `reifyLanguage.ts`, node type by
 * node type. A brace-delimited body that is its own node type and gets no
 * entry there parses perfectly cleanly and silently loses its fold arrow and
 * its delimited indent — a failure EVERY other assertion in this file is blind
 * to, because they all count error nodes or read node names.
 *
 * So the invariant gets the same data-driven treatment as the `kw<>` →
 * KEYWORDS guard above: derive the brace-delimited node types from the grammar
 * SOURCE and require the two lists to cover them.
 */
describe('reifyLanguage — fold and indent coverage', () => {
  /**
   * `Interpolation` (`"{" expression "}"`) is brace-delimited but is a string
   * HOLE, not a body: folding it would hide the enclosing string's content and
   * it lives in the grammar's no-skip context. Excluded deliberately, and named
   * here so the exclusion is a decision rather than an omission.
   */
  const EXCLUDED_BRACE_NODES = ['Interpolation'];

  /**
   * Every capitalised production in reify.grammar whose own body contains a
   * literal `"{"` token. Scans line by line, tracking the most recent
   * production header, and stops at `@tokens` — inside that block `"{"` is a
   * token declaration, not a body.
   */
  function braceDelimitedNodeTypes(grammarSrc: string): string[] {
    const found = new Set<string>();
    let current: string | null = null;
    for (const rawLine of grammarSrc.split('\n')) {
      if (/^@tokens\b/.test(rawLine)) break;
      // Strip line comments so prose about braces never counts as a body.
      const line = rawLine.replace(/\/\/.*$/, '');
      const header = line.match(/^\s*([A-Z][A-Za-z0-9_]*)\s*\{/);
      if (header) current = header[1];
      if (current && line.includes('"{"')) found.add(current);
    }
    return [...found].sort();
  }

  it('gives every brace-delimited body a fold and an indent entry', () => {
    const declared = braceDelimitedNodeTypes(readFixture('gui/src/editor/reify.grammar'));
    // Sanity: an empty or tiny extraction must not let the check pass
    // vacuously. `Block` is the oldest brace-delimited body in the grammar.
    expect(declared.length).toBeGreaterThan(10);
    expect(declared).toContain('Block');

    const covered = new Set([...BRACE_FIRST_BODIES, ...KEYWORD_LED_BODIES, ...EXCLUDED_BRACE_NODES]);
    const missing = declared.filter((name) => !covered.has(name));
    expect(
      missing,
      `These node types are brace-delimited bodies in reify.grammar but appear ` +
        `in neither BRACE_FIRST_BODIES nor KEYWORD_LED_BODIES in ` +
        `gui/src/editor/reifyLanguage.ts, so they fold and indent as if they ` +
        `were not blocks at all. Add each to the list that matches whether its ` +
        `\`{\` is the FIRST child, in the same commit as the production.`,
    ).toEqual([]);
  });

  /**
   * The reverse direction: a list entry naming a node type the grammar no
   * longer declares is dead configuration that the check above cannot see.
   */
  it('lists no node type the grammar does not declare', () => {
    const declared = new Set(braceDelimitedNodeTypes(readFixture('gui/src/editor/reify.grammar')));
    const stale = [...BRACE_FIRST_BODIES, ...KEYWORD_LED_BODIES].filter((n) => !declared.has(n));
    expect(stale).toEqual([]);
  });

  /**
   * And the props actually RESOLVE on the configured parser. The list check
   * above compares two string arrays; it would stay green if `parser.configure`
   * silently dropped the props (a rename of `foldNodeProp`, a node type that
   * exists in the grammar but not in the generated node set). This reads the
   * props back off the exact parser object the editor drives.
   */
  it.each([...BRACE_FIRST_BODIES, ...KEYWORD_LED_BODIES])(
    'resolves fold and indent props on %s',
    (name) => {
      const nodeType = reifyLRLanguage.parser.nodeSet.types.find((t) => t.name === name);
      expect(nodeType, `${name} is not a node type in the generated parser`).toBeDefined();
      expect(nodeType!.prop(foldNodeProp)).toBeDefined();
      expect(nodeType!.prop(indentNodeProp)).toBeDefined();
    },
  );

  /**
   * The raw body text of production `name` in `grammarSrc` — everything
   * between its own header brace and the matching close, comments stripped,
   * newlines collapsed to single spaces. Braces inside a quoted token
   * (`"{"`/`"}"`, the LITERAL tokens the grammar matches) never affect the
   * depth count; only the bare rule-delimiter braces do, so a production
   * whose body itself contains several `"{"`/`"}"` pairs (`FieldSource`, say)
   * still finds the RULE's own close rather than stopping at the first
   * quoted `"}"`.
   */
  function productionBody(grammarSrc: string, name: string): string {
    const src = grammarSrc.replace(/\/\/.*$/gm, '');
    const header = new RegExp(`(^|\\n)${name}\\s*\\{`).exec(src);
    if (!header) throw new Error(`production ${name} not found in reify.grammar`);
    let depth = 1;
    const start = header.index + header[0].length;
    for (let i = start; i < src.length; i++) {
      if (src.startsWith('"{"', i) || src.startsWith('"}"', i)) {
        i += 2;
        continue;
      }
      if (src[i] === '{') depth++;
      else if (src[i] === '}') {
        depth--;
        if (depth === 0) return src.slice(start, i).replace(/\s+/g, ' ').trim();
      }
    }
    throw new Error(`unbalanced braces in production ${name}`);
  }

  /**
   * `body` split on its TOP-LEVEL `|` alternatives — paren-depth aware, so a
   * NESTED `|` (`PortBody`'s `(ParamDeclaration | LetDeclaration | …)*`, say)
   * does not fracture one arm into two.
   */
  function splitTopLevelArms(body: string): string[] {
    const arms: string[] = [];
    let depth = 0;
    let start = 0;
    for (let i = 0; i < body.length; i++) {
      if (body.startsWith('"{"', i) || body.startsWith('"}"', i) || body.startsWith('"|"', i)) {
        i += 2;
        continue;
      }
      const ch = body[i];
      if (ch === '(') depth++;
      else if (ch === ')') depth--;
      else if (ch === '|' && depth === 0) {
        arms.push(body.slice(start, i));
        start = i + 1;
      }
    }
    arms.push(body.slice(start));
    return arms.map((arm) => arm.trim());
  }

  /**
   * The BRACE_FIRST_BODIES/KEYWORD_LED_BODIES split is only observable
   * downstream through the indent prop — `foldBody` above is the SAME
   * function for both lists (reifyLanguage.ts:115-118), so every fold
   * assertion in this file passes unchanged even if a node type sits in the
   * wrong list. What actually differs is `delimitedIndent`'s `align`
   * (reifyLanguage.ts:119-127): a keyword-led body misfiled into
   * BRACE_FIRST_BODIES would align its members to the column past the
   * leading keyword instead of one unit past the statement. This derives the
   * correct list from the grammar SOURCE directly — the same evidence
   * `braceDelimitedNodeTypes` above uses for list membership itself, read
   * positionally instead of just for presence.
   */
  describe('BRACE_FIRST_BODIES vs KEYWORD_LED_BODIES matches the grammar', () => {
    const grammarSrc = readFixture('gui/src/editor/reify.grammar');

    it.each(BRACE_FIRST_BODIES)('%s opens its braced arm with the brace itself', (name) => {
      const arms = splitTopLevelArms(productionBody(grammarSrc, name));
      const braceArms = arms.filter((arm) => arm.includes('"{"'));
      expect(braceArms.length, `${name} has no "{" token in its reify.grammar production at all`).toBeGreaterThan(
        0,
      );
      expect(
        braceArms.every((arm) => arm.startsWith('"{"')),
        `${name} is in BRACE_FIRST_BODIES, but at least one of its arms in reify.grammar has ` +
          `content before the "{" — it belongs in KEYWORD_LED_BODIES instead. Arms: ${JSON.stringify(braceArms)}`,
      ).toBe(true);
    });

    it.each(KEYWORD_LED_BODIES)('%s never opens an arm with a bare brace', (name) => {
      const arms = splitTopLevelArms(productionBody(grammarSrc, name));
      const braceFirstArms = arms.filter((arm) => arm.startsWith('"{"'));
      expect(
        braceFirstArms,
        `${name} is in KEYWORD_LED_BODIES, but reify.grammar opens at least one of its arms ` +
          `directly with "{" — it belongs in BRACE_FIRST_BODIES instead.`,
      ).toEqual([]);
    });
  });

  /**
   * The behavioural half. `foldInside` — the obvious helper, and the one this
   * file used before the keyword-led bodies were added — keys off the FIRST
   * child, which is the `{` only when the brace opens the node. For
   * `enum Color { … }` the first child is `enum`, so `foldInside` would start
   * the fold right after `enum` and hide the enum's NAME. This pins that the
   * fold range starts at the body brace instead.
   */
  it('folds a keyword-led body from its own brace, not from the leading keyword', () => {
    const src = 'enum Color {\n  Red,\n  Green\n}';
    const cursor = reifyLRLanguage.parser.parse(src).cursor();
    let decl: SyntaxNode | null = null;
    do {
      if (cursor.type.name === 'EnumDeclaration') decl = cursor.node;
    } while (!decl && cursor.next());
    expect(decl, 'no EnumDeclaration in the parse').not.toBeNull();

    const fold = decl!.type.prop(foldNodeProp)!;
    const range = fold(decl!, EditorState.create({ doc: src }));
    // The exact range is now also covered by the `folds EnumDeclaration to a
    // non-null range spanning its own brace interior` case in the it.each
    // below (same node, same call shape), so this test keeps only the claim
    // that case does not name: the fold must not start at or before the
    // enum's own NAME — folding from `enum` instead of from `{` (what
    // `foldInside` would do) would hide `Color` inside the fold arrow.
    expect(range, 'foldNodeProp returned null').not.toBeNull();
    expect(range!.from).toBeGreaterThan(src.indexOf('Color'));
  });

  /**
   * THE BEHAVIOURAL GUARD task #6325 was filed for. `resolves fold and indent
   * props on %s` above only checks that `foldNodeProp` is DEFINED on the node
   * type — a prop that resolves and then returns `null` at CALL time (which is
   * exactly what `foldBody` does whenever `node.getChild('{')` finds nothing)
   * passes that check while the fold arrow is dead. Task #5931 measured this
   * for real: folding `ImportItems`' opener into an anonymous literal token
   * left `foldBody` returning `null` for `import std.mech.{Bolt, Nut}` while
   * the whole 428/428 GUI suite stayed green, because nothing called the prop
   * and looked at what came back.
   *
   * `BEHAVIOR_FIXTURES` pins one minimal, corpus-attested-where-possible
   * snippet per node type in `BRACE_FIRST_BODIES`/`KEYWORD_LED_BODIES`, and the
   * test below actually invokes `foldNodeProp` on the located node — through
   * `EditorState.create`, the same call shape the editor makes — and asserts
   * the range is non-null and spans exactly the body's own brace interior.
   *
   * `FnBody` and the `JointDof`/`JointBody` family each have a second,
   * legitimately brace-less arm (`= expression`; a bare DOF field or a bare
   * expression body) where `foldBody` returning `null` is the CORRECT answer,
   * not a gap. This map pins only the BRACED arm for those three, so the
   * `it.each` below asserts non-null uniformly across every entry; the
   * brace-less arms get their own explicit expected-null test right after it,
   * so that half of the contract is pinned too rather than left implicit.
   */
  // `FieldDefinition` and `FieldSource` are two node types nested in the SAME
  // production (`FieldDefinition`'s body is exactly one `source = <FieldSource
  // arm>`, reify.grammar:36-39) so one fixture string locates both; hoisted to
  // a shared const rather than duplicated so editing one cannot silently
  // change which node the OTHER entry targets.
  const FIELD_DEFINITION_SRC = 'field def top_layer : Real -> Real { source = analytical { |x| 100.0 } }';

  /**
   * One minimal, corpus-attested-where-possible source snippet per node type.
   * A plain `Record<string, string>` rather than a one-field wrapper object —
   * every entry is written and consumed as a bare source string, so a wrapper
   * would buy nothing over the map itself.
   */
  const BEHAVIOR_FIXTURES: Record<string, string> = {
    // ── BRACE_FIRST_BODIES ──────────────────────────────────────────────
    Block: 'structure def F { let x = 1mm }',
    // Corpus-attested VERBATIM: the braced-arm slice above (`fn g() -> Real
    // { let a = 1.0; a }`).
    FnBody: 'fn g() -> Real { let a = 1.0; a }',
    SpecializationBody: 'structure def S { sub x : Foo<T> { bore = 5mm } }',
    PortBody: 'structure def F { port inlet : in FluidPort { param diameter : Length = 25mm } }',
    ConnectBody:
      'structure def F { connect outlet -> inlet { diameter -> diameter, flow_rate -> flow_rate } }',
    // `import a.b {C, D}` — no separator, matching the RULE this port follows
    // (see the long note on `ImportDeclaration` in reify.grammar).
    ImportItems: 'import std.mech {Bolt, Nut}',
    // Corpus-attested VERBATIM: examples/keyed_vents.ri:27-30.
    KeyedMemberBlock:
      'structure def S { sub vents : Keyed<Vent> { "intake" => { area = 5mm }  "exhaust" => { area = 8mm } } }',
    // Corpus-attested VERBATIM: examples/joint_dof_self_check.ri:32 — the
    // braced-record DOF form.
    JointDof:
      'joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } = concentric(a, b)',
    // Corpus-attested VERBATIM: examples/joint_dof_self_check.ri:26 — the
    // block-arm JointBody.
    JointBody:
      'joint revolute(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) with angle: Angle in 0deg..120deg = { concentric(a, b)  on(p, stop) }',
    // ── KEYWORD_LED_BODIES ──────────────────────────────────────────────
    MatchExpression:
      'structure def F { let code = match grade { Standard => 1, Reinforced => 2, Premium => 3 } }',
    EnumDeclaration: 'enum Grade { Standard, Reinforced, Premium }',
    // First EnumVariant in this attested string (`Circle { radius: Length }`)
    // carries the braced payload; the sibling `Point` variant is bare and is
    // not what `findFirstNodeNamed` locates here.
    EnumVariant: 'enum Shape { Circle { radius: Length }, Point }',
    MetaBlock: 'structure def F { meta { project = "integration-test", version = "0.1" } }',
    SetLiteral: 'structure def F { let s = set { 1, 2 } }',
    MapLiteral: 'structure def F { let m = map { 1 => 2 } }',
    VariantConstruction: 'structure def F { let s = Circle { radius: 5mm } }',
    // First VariantBindingPattern in this attested string is `Circle { radius: r }`.
    VariantBindingPattern:
      'structure def F { let area = match outline { Circle { radius: r } => 3.14159 * r * r, Rect { width: w, height: h } => w * h, Point => 0mm * 0mm } }',
    FieldDefinition: FIELD_DEFINITION_SRC,
    FieldSource: FIELD_DEFINITION_SRC,
    PurposeDeclaration: 'purpose design_review(subject : Structure) { constraint 1mm > 0mm }',
    RelateBlock: 'structure def F { relate { fasten(a.frame, b.frame) } }',
    ConstraintDefinition: 'constraint def MinThickness {\n    param t: Length\n    t > 1mm\n}',
    SubRelateBlock: 'structure def F { sub s : T at auto where { concentric(a, b) } }',
    MatchArmDeclBlock:
      'structure def F { match kind { Hex => sub head : HexHead, Sq => sub head : SqHead } }',
  };

  it('pins exactly one behavioural fixture per brace-delimited body', () => {
    // Same drift protection as the two list-vs-grammar checks above, aimed at
    // THIS map instead: a node type added to either list with no fixture here
    // would silently skip the behavioural assertion below rather than fail it.
    expect(Object.keys(BEHAVIOR_FIXTURES).sort()).toEqual(
      [...BRACE_FIRST_BODIES, ...KEYWORD_LED_BODIES].sort(),
    );
  });

  /** First node named `name` in the parse of `src`, or `null` if none. */
  function findFirstNodeNamed(src: string, name: string): SyntaxNode | null {
    const cursor = reifyLRLanguage.parser.parse(src).cursor();
    do {
      if (cursor.type.name === name) return cursor.node;
    } while (cursor.next());
    return null;
  }

  /**
   * The brace pair belonging to `node` itself, located by SCANNING THE SOURCE
   * TEXT within `node`'s own span rather than by reading `node`'s children —
   * so this is an independent check of what `foldNodeProp` returns, not a
   * restatement of `foldBody`'s own `getChild('{')` logic against itself.
   * `node.to - 1` bounds the close search so a NESTED body's closing brace
   * (an override block inside a keyed entry, say) is never mistaken for
   * `node`'s own.
   *
   * The open is NOT simply "the first `{` at or after `node.from`" — a
   * keyword-led production's HEADER can itself contain a brace pair before
   * the body's own: `MatchExpression`'s discriminant is a full `expression`
   * (reify.grammar:932), so `match set { 1 } { A => 1 }` is legal, and the
   * naive first-`{` reading would return the scrutinee's `set { 1 }` interior
   * instead. Scanning forward from `node.from` with a brace-depth counter and
   * taking the position of the LAST 0→1 transition reached before `close`
   * finds the brace that actually PAIRS WITH `close` — i.e. `node`'s own —
   * regardless of how many complete brace pairs precede it in the header.
   */
  function ownBraceInterior(src: string, node: SyntaxNode): { from: number; to: number } {
    const close = src.lastIndexOf('}', node.to - 1);
    if (close < 0) {
      throw new Error(`no closing brace inside node span [${node.from}, ${node.to}) of: ${src}`);
    }
    let depth = 0;
    let open = -1;
    for (let i = node.from; i <= close; i++) {
      if (src[i] === '{') {
        if (depth === 0) open = i;
        depth++;
      } else if (src[i] === '}') {
        depth--;
      }
    }
    if (open < 0 || open >= close) {
      throw new Error(`no brace pair inside node span [${node.from}, ${node.to}) of: ${src}`);
    }
    return { from: open + 1, to: close };
  }

  it.each(Object.entries(BEHAVIOR_FIXTURES))(
    'folds %s to a non-null range spanning its own brace interior',
    (name, src) => {
      expect(countErrorNodes(src), `${name} fixture has error nodes: ${src}`).toBe(0);

      const node = findFirstNodeNamed(src, name);
      expect(node, `no ${name} node in the parse of: ${src}`).not.toBeNull();

      const fold = node!.type.prop(foldNodeProp)!;
      const range = fold(node!, EditorState.create({ doc: src }));
      expect(range, `${name}'s foldNodeProp returned null on: ${src}`).not.toBeNull();
      expect(range).toEqual(ownBraceInterior(src, node!));
    },
  );

  /**
   * The other half of the contract: the brace-less arms and the empty-body
   * case, where `null` IS the correct answer, so the non-null assertion above
   * does not simply invert into a false failure for them. `FnBody`'s
   * `= expression` sugar (grammar.js:239-246) and `JointDof`/`JointBody`'s
   * bare-field / bare-expression arms (grammar.js:781-784, 799-802) are the
   * only three legitimately brace-LESS arms — pinned by the two `it`s below.
   *
   * A fourth path returns `null` for a different reason: it HAS a brace pair,
   * but nothing between them. `foldBody`'s `open.to >= close.from` guard
   * (reifyLanguage.ts:83) fires for any empty body, braced or not — there is
   * nothing to fold — and it is corpus-attested, not merely hypothetical:
   * `port inlet : in Flow {}` (examples/keyed_vents.ri:23) is an empty
   * `PortBody`. Pinned separately below via `SetLiteral` so the assertion
   * stays a minimal, self-contained snippet.
   */
  describe('legitimately brace-less arms return null', () => {
    it('FnBody `= expression` sugar', () => {
      // Corpus-attested VERBATIM: the brace-less-arm slice above.
      const src = 'fn twice(x : Real) -> Real = x * 2';
      expect(countErrorNodes(src)).toBe(0);
      const node = findFirstNodeNamed(src, 'FnBody');
      expect(node, 'no FnBody in the parse').not.toBeNull();
      const fold = node!.type.prop(foldNodeProp)!;
      expect(fold(node!, EditorState.create({ doc: src }))).toBeNull();
    });

    it('JointDof bare field and JointBody bare expression', () => {
      // Corpus-attested VERBATIM: examples/joint_dof_self_check.ri — the
      // range-less-DOF slice above. Exercises both node types' brace-less arm
      // in one snippet: the DOF is a bare `JointDofField`, and the body is the
      // bare expression `concentric(a, b)`.
      const src = 'joint prismatic(a: Axis, b: Axis) with travel: Length = concentric(a, b)';
      expect(countErrorNodes(src)).toBe(0);
      for (const name of ['JointDof', 'JointBody']) {
        const node = findFirstNodeNamed(src, name);
        expect(node, `no ${name} in the parse`).not.toBeNull();
        const fold = node!.type.prop(foldNodeProp)!;
        expect(fold(node!, EditorState.create({ doc: src }))).toBeNull();
      }
    });

    it('empty body — nothing between the braces to fold', () => {
      const src = 'structure def F { let s = set {} }';
      expect(countErrorNodes(src)).toBe(0);
      const node = findFirstNodeNamed(src, 'SetLiteral');
      expect(node, 'no SetLiteral in the parse').not.toBeNull();
      const fold = node!.type.prop(foldNodeProp)!;
      expect(fold(node!, EditorState.create({ doc: src }))).toBeNull();
    });
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
// MEASUREMENT (task 5927), the catch-up round that inherited this ledger.
// Fifteen form families were ported, each in its own commit that extended the
// pinned set below, taking it from 90 to 301 of the same 329 files:
//
//   named call arguments · collection literals + index access · keyword
//   logical operators · range expressions · type parameters + trait bounds ·
//   the rich `sub` forms · unit expressions + value-level `^` · trait and fn
//   declarations · lambdas + `@` selectors · parameterized `auto` +
//   auto_type_arg · port/connect/chain/forall + the quantifier expression ·
//   match/enum/variant construction · field def/purpose/unit/type alias/
//   default/meta/pragma/annotation · radix/imaginary/interpolated literals
//
// That round left the set at 302 paths, not 301, and the extra one was NOT a
// grammar gain: the corpus itself grew to 330 files while that task was in
// flight (`examples/whole_model_cost_min.ri`), and the new file parses clean.
// Its final measurement is therefore 302 / 330. The fifteenth family is worth
// zero files — radix, imaginary and interpolated literals already parsed with
// zero error nodes under the `QuantityLiteral` / `String` catch-alls, so that
// slice was taken for node SHAPE (an editor cannot style what it cannot name)
// and is guarded by explicit non-regression assertions above rather than by
// any movement here.
//
// MEASUREMENT (task 5941), catch-up round 3. Five families were ported, taking
// the pinned set from 302 to 309 of 330 — and the arithmetic is worth stating
// plainly, because only TWO of the five moved it at all:
//
//   relate blocks              +5   the whole ledger movement, less two
//   constraint def             +2
//   variant construction in argument position   0
//   sub_relate_block                            0
//   match_arm_decl_block                        0
//
// The three zero-movement families are taken for node SHAPE, the same
// justification 5927 recorded for the literal families, and are guarded by the
// node-name assertions in their slices above rather than by any movement here.
// For two of them that guarding is the ONLY guarding there is, because the
// wrong reading parses clean: an argument-position construction could have been
// widened into `primaryExpression` and silently swallowed a `match`/`where`/
// `connect` body, and a `sub_relate_block` resolved with precedence instead of
// the `~poseWhere` marker would silently have stolen every post-pose `where`
// from `GuardedBlock`. Neither would have shown up as an error node.
//
// FOUR OF THE SIX `constraint def` FILES DID NOT TURN CLEAN, and their absence
// below is not a regression. Each keeps a second, out-of-scope blocker — in
// order: `some(u)`, `meta.material`, `meta.project`, and a member-level
// `@annotation`. Their error counts did drop (integration_corner_cases 9→7,
// integration_full_v01 14→5, m11_annotations 18→10, m9_combined 9→4), which is
// real progress that this pinned set is deliberately unable to express.
//
// MEASUREMENT (task 5950), catch-up round 4. Round 3 handed on twelve families,
// derived from each not-clean file's FIRST blocker. Ten of them were real and
// were ported; one is owned elsewhere; two did not exist. The pinned set moved
// from 309 to 327 of 330:
//
//   `chain` demoted to `ekw<>`                 +2
//   a `ReservedWord` in CALL position          +3
//   the `%` operator                           +1
//   `priv param`                               +1
//   `auto` as a NAMED-ARGUMENT value           +1
//   a member-level `Annotation`                +1
//   `meta` demoted to `ekw<>`                  +2
//   `::` in EXPRESSION position                +3
//   `keyed_member_block`                       +2
//   `joint_definition`                         +1
//   `·` unit multiplication                    +1
//                                             ────
//                                              +18
//
// TWO OF THOSE CREDITS ARE THE LAST OF TWO CHANGES, not the only one, and the
// column would mislead without saying so: integration_corner_cases.ri needed
// BOTH `chain` and the `ReservedWord` callee, and integration_full_v01.ri
// needed BOTH the callee and `meta`. Each file is credited to the change that
// FINISHED it. A per-family delta cannot be additive when a file has two
// blockers; naming the two is cheaper than implying they were independent.
//
// WHERE ROUND 3'S FORECAST WAS WRONG — three of its twelve entries, all three
// found by measuring rather than by trusting the list, and all three now pinned
// by tests above so the next round does not re-litigate them:
//
//   - "two stackup files" was not a list-literal gap. It was `chain`, which
//     round 2 had promoted to a hard `kw<>`; that is context-free in Lezer, so
//     `chain` stopped being a legal identifier in three committed files. A
//     self-inflicted regression, and the cheapest fix in the round.
//   - "three remaining one-offs" was two. Chained comparison `a < b < c < d`
//     already parsed with zero errors in both binding and constraint position;
//     integration_corner_cases.ri:131 is `let chain = …`, so its errors were
//     the keyword above and :133-135 were cascade.
//   - "arrow (function) types in a param annotation" was already reachable —
//     `FunctionType` has been in the grammar all along.
//
// WHY THE LAST THREE FILES REMAIN — one line each, because "27 short of the
// corpus" is not a finding and "which three, and whose" is:
//
//   - stdlib_ns_qualified_expr.ri  ) `pp.Pulley` binding-qualified references.
//   - stdlib_ns_qualified_type.ri  ) LANDED by #5495 μ, which round 4 named as
//     the only inheritable work IT HAD IDENTIFIED. Both are now in the ledger
//     below. The node shapes the two new productions must produce — and the
//     controls they must not disturb — are pinned in
//     reifyGrammarQualifiedRef.test.ts, the file that task owns; this ledger
//     records only that the two fixtures parse clean.
//   - arrow_type.ri — NOT A GRAMMAR GAP AT ALL. Its `param` is at TOP LEVEL,
//     and `param` is not a top-level declaration in tree-sitter's
//     `_declaration` (grammar.js:134) or in the compiler's `lower_source_file`
//     (crates/reify-syntax/src/ts_parser.rs:299). Admitting it here would make
//     the editor accept what neither of those does — a third dialect. The
//     fixture's own header declares it a deliberate FAIL probe. Both halves are
//     pinned above: the member-position arrow type IS clean, and the top-level
//     `param` MUST error.
//
// THE ARITHMETIC ABOVE IS INHERITED AND ITS DENOMINATOR IS STALE. Every "of
// 330" in this block dates from an earlier round's corpus inventory; #5495 μ
// re-measured it through this test's own walk and `countErrorNodes`, and the
// corpus is bigger than the block assumes:
//
//     361 committed .ri under CORPUS_ROOTS · 358 parse clean · 329 pinned below
//
// So arrow_type.ri is NOT the only un-pinned file: 29 currently-clean files sit
// outside the ratchet — examples/best_practices/angle_crossings.ri, the ten
// tests/prd-gate/fixtures/pnrg_envelope_*.ri, compose_fn_field_resolves.ri and
// the rest. That gap is real inheritable work and is the reason this comment no
// longer claims completeness: a grammar change could regress any of those 29
// and the ledger — the artifact whose whole purpose is catching exactly that —
// would stay green.
//
// DO NOT RE-STATE THOSE THREE NUMBERS AS THE NEW TRUTH. They were measured on
// 2026-08-23 and go stale the next time anyone adds a `.ri`, which is how the
// "330" above rotted in the first place. The LIVE arithmetic is printed by this
// test's own failure message (`measured N clean of M …; K clean files are not
// pinned`); to see it on demand, read those three counts off a run rather than
// off this comment. A future round adding a family should shrink the 29 and
// leave the counting to the test.
//
// AND NOTE WHAT THIS BLOCK DELIBERATELY DOES NOT CONTAIN. The per-production
// reasoning for every change above — why `ekw<>` and not `kw<>`, why the
// callee and not the `ReservedWord` list, why `·` cannot escape `unitOp` —
// lives on the productions themselves in reify.grammar. #5941 was amended
// TWICE to stop this file restating them (`eb2394f2cd`, `05e5c50eac`); read
// the grammar, not a second copy that drifts.
//
// `variant_construction` left round 3's inherited list before round 4 began:
// argument position is now admitted.
// The narrowing that remains is `primaryExpression` only, it is deliberate, and
// the note on that production in reify.grammar prices it — see also the pin
// above asserting that expression-position `Name { … }` must STILL error.
//
// AND ONE KNOWN DIVERGENCE, which is not a gap and must not be "fixed" blindly.
// Underscore-separated DECIMAL literals did not get their own node: `1_000_000`
// still lexes as `QuantityLiteral` (`1` + unit `_000_000`). The companion-token
// trick that made `0xDEAD_BEEF` one `RadixLiteral` cannot be reused, because
// lezer token precedence beats longest match: a separator-aware `Number`
// ordered above `QuantityLiteral` takes `1_000` out of the ATTESTED `1_000mm`
// (examples/numeric_separators.ri:17) and strands the `mm`, turning a
// committed line into an error node. Zero ledger impact either way; pinned by
// shape tests above so it stays visible. See the fuller accounting on the
// `Number` token in reify.grammar, which prices the full trade.
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
// addition here. #5950 turned that ratchet to 327 entries; #5495 μ added its two
// fixtures. The entries below are a SUBSET of the clean set, not the whole of
// it — see the re-measured arithmetic above and the 29-file gap it names.
const EXPECTED_CLEAN = [
  'examples/ad_hoc_face_selector.ri',
  'examples/affine_tapered_spacer.ri',
  'examples/ambient_default_material/ambient_default_surface.ri',
  'examples/anisotropic_bar.ri',
  'examples/appearance_surface.ri',
  'examples/appearance_viewport_egress.ri',
  'examples/aspect_massive.ri',
  'examples/auto/bearing_computed_default_unevaluated.ri',
  'examples/auto/bearing_constraint_select.ri',
  'examples/auto/bearing_resolved_value.ri',
  'examples/auto/bearing_unsat.ri',
  'examples/auto/bounded_fallback_unsound.ri',
  'examples/auto_binding_sites.ri',
  'examples/bearing_auto_seal.ri',
  'examples/best_practices/bolt_circle.ri',
  'examples/best_practices/clearance_oracle.ri',
  'examples/best_practices/discrete_choice.ri',
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
  'examples/conditional_compilation/main.ri',
  'examples/conditional_compilation/platform_linux.ri',
  'examples/conditional_compilation/platform_wasm.ri',
  'examples/continuous_cost_min.ri',
  'examples/cost_aggregation.ri',
  'examples/cost_robustness_tradeoff.ri',
  'examples/cost_subtree_aggregate.ri',
  'examples/datum_projections.ri',
  'examples/determinacy_intrinsics.ri',
  'examples/differential_field_ops.ri',
  'examples/dimensional_chains.ri',
  'examples/dimensional_consistency.ri',
  'examples/dimensionless_unification.ri',
  'examples/drivebelt_trait_bounds.ri',
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
  'examples/fea_shell_channels.ri',
  'examples/fea_shell_flexure.ri',
  'examples/fea_shell_too_thick_annotated.ri',
  'examples/fea_shell_too_thick_auto.ri',
  'examples/fields/compose.ri',
  'examples/fields/composed_stiffness.ri',
  'examples/fields/fn_field.ri',
  'examples/fields/from_samples.ri',
  'examples/fields/pointwise_field_combinators.ri',
  'examples/fields/restrict.ri',
  'examples/fields/spatial_ops.ri',
  'examples/fields/std_fields_surface.ri',
  'examples/fields_analysis.ri',
  'examples/flexures/cantilever_beam_prb.ri',
  'examples/flexures/double_parallelogram.ri',
  'examples/flexures/notch_hinge_circular_prb.ri',
  'examples/flexures/parallelogram_stage.ri',
  'examples/flexures/printer_z_compliant_mount.ri',
  'examples/flexures/yield_warning.ri',
  'examples/gdt_conformance_satisfied.ri',
  'examples/gdt_conformance_violated.ri',
  'examples/generate_bolt_circle.ri',
  'examples/generics/container.ri',
  'examples/generics/dim_param.ri',
  'examples/generics/identity.ri',
  'examples/generics/unbound_param.ri',
  'examples/geometric_relations/bolt_plate.ri',
  'examples/geometric_relations/construction_datum.ri',
  'examples/geometric_relations/feature_datum_axis.ri',
  'examples/geometric_relations/global_float.ri',
  'examples/half_space.ri',
  'examples/imported_field/openvdb_stress.ri',
  'examples/integration_corner_cases.ri',
  'examples/integration_full_v01.ri',
  'examples/interpolation.ri',
  'examples/io_export.ri',
  'examples/io_formats.ri',
  'examples/joint_dof_self_check.ri',
  'examples/kernel_queries/adjacent_faces.ri',
  'examples/kernel_queries/all_queries_walk.ri',
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
  'examples/kernel_queries/moment_of_inertia_box.ri',
  'examples/kernel_queries/normal_smoke.ri',
  'examples/keyed_forall.ri',
  'examples/keyed_vents.ri',
  'examples/kinematic/counter_mass_balance.ri',
  'examples/kinematic/dock_pickup.ri',
  'examples/kinematic/four_bar_singular.ri',
  'examples/kinematic/relate_mounted_fourbar.ri',
  'examples/kinematic/relate_mounted_revolute.ri',
  'examples/kinematic/revolute_pivot_offset.ri',
  'examples/kinematic/spatial_linkage_oriented.ri',
  'examples/kleene_e2e.ri',
  'examples/large_assembly.ri',
  'examples/linalg.ri',
  'examples/list_helpers.ri',
  'examples/litter_tray.ri',
  'examples/load_case.ri',
  'examples/m10_combined.ri',
  'examples/m10_connect_advanced.ri',
  'examples/m10_geometric_types.ri',
  'examples/m10_purpose_activation.ri',
  'examples/m11_annotations.ri',
  'examples/m11_field_calculus.ri',
  'examples/m5_collection_ops.ri',
  'examples/m5_combined_all.ri',
  'examples/m5_connect_chain.ri',
  'examples/m5_function_safety_factor.ri',
  'examples/m5_geometry.ri',
  'examples/m5_geometry_flange.ri',
  'examples/m5_guarded_enum.ri',
  'examples/m5_guarded_head_type.ri',
  'examples/m5_occurrence_process.ri',
  'examples/m5_purpose.ri',
  'examples/m5_trait_rigid.ri',
  'examples/m5_trait_structure.ri',
  'examples/m5_user_function.ri',
  'examples/m6_data_carrying_enum.ri',
  'examples/m6_data_carrying_enum_undef.ri',
  'examples/m6_fallback_recovery.ri',
  'examples/m6_generic_enum.ri',
  'examples/m6_result_fallback.ri',
  'examples/m6_result_recovery.ri',
  'examples/m8_materials.ri',
  'examples/m8_ports.ri',
  'examples/m8_tolerancing.ri',
  'examples/m8_units.ri',
  'examples/m9_combined.ri',
  'examples/m9_constraint_def.ri',
  'examples/m9_determinacy.ri',
  'examples/m9_integration.ri',
  'examples/m9_trait_conformance.ri',
  'examples/material_appearance_library.ri',
  'examples/materials_starter_library.ri',
  'examples/math_linalg.ri',
  'examples/modal/cantilever_beam_modes.ri',
  'examples/modal/printer_gantry_modes.ri',
  'examples/modal/simply_supported_beam_modes.ri',
  'examples/modal/transient_step_response.ri',
  'examples/module_visibility/consumer.ri',
  'examples/module_visibility/mismatch_variant.ri',
  'examples/module_visibility/producer.ri',
  'examples/modulo_index_wrap.ri',
  'examples/multi_aspect_objective.ri',
  'examples/multi_aspect_objective_mixed.ri',
  'examples/multi_kernel/attribute_selectors.ri',
  'examples/multi_kernel/manifold_boolean.ri',
  'examples/multi_kernel/pragma_override.ri',
  'examples/multi_kernel/voxel_to_mesh.ri',
  'examples/multi_kernel/voxel_to_mesh_iso.ri',
  'examples/multi_load_bracket.ri',
  'examples/multi_pane_viewport.ri',
  'examples/numeric_and_range_literals.ri',
  'examples/numeric_separators.ri',
  'examples/objective_inheritance.ri',
  'examples/option_map_or.ri',
  'examples/parametric_rate_cross_module.ri',
  'examples/parametric_vec3_cross_module.ri',
  'examples/pattern_arbitrary_transforms.ri',
  'examples/pattern_composition.ri',
  'examples/perforated_plate.ri',
  'examples/process/std_process_dfm.ri',
  'examples/process/std_process_dfm_metrology.ri',
  'examples/process/std_process_dfm_thickness.ri',
  'examples/radix_literals.ri',
  'examples/representation_within.ri',
  'examples/rigid_mass_props_smoke.ri',
  'examples/selectors/relational_selectors_v2.ri',
  'examples/selectors/selector_vocabulary_v2_leaves.ri',
  'examples/selectors/single_face_by_normal.ri',
  'examples/selectors/vertices_index_coercion.ri',
  'examples/shells/thin_walled_bracket.ri',
  'examples/single_sided_range.ri',
  'examples/solid-param-direct.ri',
  'examples/solver_optimality_unproven.ri',
  'examples/spec-shape-physical.ri',
  'examples/stdlib/constants.ri',
  'examples/stdlib/fields.ri',
  'examples/stdlib/ports_breadth.ri',
  'examples/stdlib/ports_domains.ri',
  'examples/stdlib/ports_mechanical.ri',
  'examples/stdlib/ports_prelude.ri',
  'examples/stdlib/process.ri',
  'examples/structural_query_bom.ri',
  'examples/structural_query_children_members.ri',
  'examples/structural_query_descendants.ri',
  'examples/structural_query_filter.ri',
  'examples/structural_traits_dimensioned.ri',
  'examples/structure-instance.ri',
  'examples/sub_placement_assembly.ri',
  'examples/surface_finish_3mf.ri',
  'examples/surface_finish_cost.ri',
  'examples/surface_finish_functional.ri',
  'examples/surface_finish_viewport.ri',
  'examples/sweep_degenerate.ri',
  'examples/tensegrity_cable_net.ri',
  'examples/tensegrity_membrane_formfind.ri',
  'examples/tensegrity_membrane_patch.ri',
  'examples/tensegrity_pavilion.ri',
  'examples/tensegrity_t_prism.ri',
  'examples/tolerance-stackup-3part.ri',
  'examples/tolerance-stackup-rss.ri',
  'examples/tolerancing/gdt_illegal_modifier.ri',
  'examples/tolerancing/gdt_legality_rfs.ri',
  'examples/tolerancing/gdt_oracle_inside.ri',
  'examples/tolerancing/gdt_oracle_outside.ri',
  'examples/tolerancing/gdt_pass_weave.ri',
  'examples/tolerancing/gdt_removed_2018.ri',
  'examples/tolerancing/gdt_zones.ri',
  'examples/tolerancing/std_tolerancing_surface.ri',
  'examples/tolerancing/vc_bolt_pattern_clearance.ri',
  'examples/tolerancing/vc_bolt_pattern_interference.ri',
  'examples/tolerancing/vc_boundary_solid.ri',
  'examples/topology_selectors/all_topology_selectors_wiring.ri',
  'examples/topology_selectors/block_inertia.ri',
  'examples/topology_selectors/fillet_top_edges.ri',
  'examples/trait_assoc_fn_cylinder.ri',
  'examples/trait_assoc_fn_overload.ri',
  'examples/trait_assoc_fn_static.ri',
  'examples/trait_assoc_type_material.ri',
  'examples/trait_assoc_type_qualified.ri',
  'examples/trait_hierarchy.ri',
  'examples/trajectory/ei_robustness.ri',
  'examples/trajectory/gcode_import_smoke.ri',
  'examples/trajectory/printer_print_envelope.ri',
  'examples/trajectory/tots_optimal_ptp.ri',
  'examples/trajectory/zv_shaped_ramp.ri',
  'examples/trajectory/zvd_robustness.ri',
  'examples/type_hygiene/type_hygiene_surface.ri',
  'examples/undef_self_describing.ri',
  'examples/unit_expressions.ri',
  'examples/whole_model_cost_min.ri',
  'examples/whole_model_joint_drive.ri',
  'tests/prd-gate/fixtures/bare_angle_silently_accepted.ri',
  'tests/prd-gate/fixtures/collection_expr_index_resolves.ri',
  'tests/prd-gate/fixtures/collection_sub_at_placement_rejected.ri',
  'tests/prd-gate/fixtures/collection_sub_member_cell_consumable.ri',
  'tests/prd-gate/fixtures/collection_sub_per_member_cells.ri',
  'tests/prd-gate/fixtures/collection_sub_value_position_undef_baseline.ri',
  'tests/prd-gate/fixtures/compiler_type_hygiene_integration_gate.ri',
  'tests/prd-gate/fixtures/compiler_type_hygiene_mul_scale_guard_defeat.ri',
  'tests/prd-gate/fixtures/compiler_type_hygiene_mul_vec_silent_int.ri',
  'tests/prd-gate/fixtures/compiler_type_hygiene_trait_args_silent_accept.ri',
  'tests/prd-gate/fixtures/cost_min_money_objective.ri',
  'tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri',
  'tests/prd-gate/fixtures/cross_sub_geometry_ref.ri',
  'tests/prd-gate/fixtures/dcr_dimension_rejection_channel_fires.ri',
  'tests/prd-gate/fixtures/dcr_fn_force_param_already_rejects.ri',
  'tests/prd-gate/fixtures/dcr_langsurface_crossdim_silent.ri',
  'tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_load_retype_target_resolves.ri',
  'tests/prd-gate/fixtures/dcr_material_dimension_correct.ri',
  'tests/prd-gate/fixtures/dcr_material_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_shaper_frequency_dimension_silent.ri',
  'tests/prd-gate/fixtures/dcr_solver_load_dropped_bare.ri',
  'tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri',
  'tests/prd-gate/fixtures/dcr_yield_stress_dimension_silent.ri',
  'tests/prd-gate/fixtures/engine_build_hardening_kappa_mixed_kernel_selector.ri',
  'tests/prd-gate/fixtures/expected_type_pushdown_arg.ri',
  'tests/prd-gate/fixtures/expected_type_pushdown_let.ri',
  'tests/prd-gate/fixtures/faces_by_normal_symbolic_eval_silent.ri',
  'tests/prd-gate/fixtures/forall_collection_resolves.ri',
  'tests/prd-gate/fixtures/forall_range_domain_rejected.ri',
  'tests/prd-gate/fixtures/geometry_let_selector_consumer.ri',
  'tests/prd-gate/fixtures/geometry_let_selector_consumer_edit.ri',
  'tests/prd-gate/fixtures/hand_placed_twin_two_subs_eval.ri',
  'tests/prd-gate/fixtures/indexed_sub_bare_member_resolves.ri',
  'tests/prd-gate/fixtures/indexed_sub_coll_arm_baseline.ri',
  'tests/prd-gate/fixtures/indexed_sub_forall_range_baseline.ri',
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
  'tests/prd-gate/fixtures/purpose_nested_structure.ri',
  'tests/prd-gate/fixtures/quantifier_expr_int_domain_resolves.ri',
  'tests/prd-gate/fixtures/quantifier_expr_member_access_rejected.ri',
  'tests/prd-gate/fixtures/quantifier_expr_range_domain_rejected.ri',
  'tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri',
  'tests/prd-gate/fixtures/revolute_silent_accept.ri',
  'tests/prd-gate/fixtures/scalar_codomain_mismatch.ri',
  'tests/prd-gate/fixtures/self_collection_count_redirect_rejected.ri',
  'tests/prd-gate/fixtures/single_sub_pose_resolves.ri',
  'tests/prd-gate/fixtures/stdlib_ns_buckling_mode_coexist.ri',
  'tests/prd-gate/fixtures/stdlib_ns_mode_member.ri',
  'tests/prd-gate/fixtures/stdlib_ns_mode_member_modal.ri',
  'tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri',
  'tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri',
  'tests/prd-gate/fixtures/stdlib_ns_std_nonexistent_import.ri',
  'tests/prd-gate/fixtures/stdlib_units_import_resolves.ri',
  'tests/prd-gate/fixtures/subbody_objective_ignored.ri',
  'tests/prd-gate/fixtures/transform3_unresolved.ri',
  'tests/prd-gate/fixtures/typeparam_member_access.ri',
  'tests/prd-gate/fixtures/uncons_box_no_error.ri',
  'tests/prd-gate/fixtures/unit_curated_labels_ascii.ri',
  'tests/prd-gate/fixtures/unit_middot_mul.ri',
  'tests/prd-gate/fixtures/unit_nm_torque_immediate.ri',
];

// pg-drift-dir:allow — reviewed directory walk (task 6435). This ledger is the
// one tracked consumer that globs tests/prd-gate/fixtures/ rather than naming
// leaves. It does NOT void verify.sh's "adding a fixture is inert" premise:
// the expectation below is asserted only over EXPECTED_CLEAN ∩ on-disk, so an
// unpinned new fixture shifts diagnostic counters and nothing more. The pinned
// subset IS gated — see _GUI_COUPLED_RI_FIXTURES in scripts/verify.sh and the
// PG-DRIFT-GUI scenario in tests/infra/test_verify_scope.sh.
const CORPUS_ROOTS = ['examples', 'tests/prd-gate/fixtures']; // pg-drift-dir:allow — reviewed, see above

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

    // Clean but NOT pinned — the ledger's blind spot, reported alongside the
    // regression list so the coverage gap is always a measured number rather
    // than a prose claim that goes stale (#5495 μ; the comment block above used
    // to assert completeness it did not have).
    const pinnedSet = new Set(EXPECTED_CLEAN);
    const unpinnedClean = [...cleanSet].filter((p) => !pinnedSet.has(p));

    expect(
      regressed,
      `These committed .ri files parsed with zero error nodes but no longer do — ` +
        `the grammar regressed:\n${regressed.map((p) => `  ${p}`).join('\n')}\n` +
        `(measured ${cleanSet.size} clean of ${allFiles.length} committed .ri files; ` +
        `${stillPresent.length} of the ${EXPECTED_CLEAN.length} pinned paths still exist on disk; ` +
        `${unpinnedClean.length} clean files are not pinned by this ledger)`,
    ).toEqual([]);
  }, LEDGER_TIMEOUT_MS);
});
