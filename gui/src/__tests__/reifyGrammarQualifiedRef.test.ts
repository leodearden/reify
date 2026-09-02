import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parser } from '../editor/reifyParser.js';

/**
 * Qualified references through an import binding (`pp.Pulley`) on the GUI Lezer
 * grammar — task 5495 μ, PRD `docs/prds/v0_6/stdlib-namespace.md` §3.3 NS-Q2.
 *
 * μ moves THREE grammar surfaces in lockstep: `tree-sitter-reify/grammar.js`,
 * the CST→AST lowering in `crates/reify-syntax/src/ts_parser.rs`, and this one.
 * The editor is the surface with no compiler behind it to catch a miss — a
 * qualified reference that the compiler accepts but Lezer rejects degrades
 * silently into error nodes and lost highlighting. `reifyGrammarCorpus.test.ts`
 * catches that at FILE granularity (the two prd-gate fixtures move into its
 * `EXPECTED_CLEAN` ledger alongside this file); what it cannot express is
 * WHICH NODE a qualified reference reduced to, and a shape regression that
 * keeps the error count at zero is exactly the silent kind. That is this file.
 *
 * MEASURED RED BASELINE on this branch, before the grammar productions land —
 * every qualified shape below fails, every control below already passes:
 *
 *   param p : pp.Pulley            2 error nodes
 *   param p : List<pp.Pulley>      2
 *   sub p : pp.Pulley              2
 *   sub p = pp.Pulley()            2
 *   let x = pp.compute(1)          1   (the `(1)` is absorbed into a
 *                                       `WhereClause` with a missing `where`)
 *   let x = pp.compute(1, scale: 2)  5
 *   CONTROL param p : Pulley       0
 *   CONTROL let x = obj.width      0
 *   CONTROL let x = plain(1)       0
 *   CONTROL param p : Beam::Material  0
 *   CONTROL let x = Foo::bar()     0
 */

/** Repo root, three levels up from `gui/src/__tests__/`. */
const REPO_ROOT = join(__dirname, '../../..');

// The three helpers below are the ones `reifyGrammarCorpus.test.ts` documents
// at length; they are re-declared rather than imported because a vitest test
// file exports nothing. Read that file for WHY each exists — in particular why
// a count and a span are both needed, and the `WildcardPattern > WildcardPattern`
// self-nesting (#5957) that a set-of-names assertion cannot see.

/** Parse `src` and count the error nodes the Lezer parser inserted. */
function countErrorNodes(src: string): number {
  const cursor = parser.parse(src).cursor();
  let errors = 0;
  do {
    if (cursor.type.isError) errors++;
  } while (cursor.next());
  return errors;
}

/** Parse `src` and count the nodes named `name`. */
function countNodesNamed(src: string, name: string): number {
  const cursor = parser.parse(src).cursor();
  let count = 0;
  do {
    if (cursor.type.name === name) count++;
  } while (cursor.next());
  return count;
}

/**
 * Node type names of every node whose span is EXACTLY the first occurrence of
 * `text` in `src`, outermost first. Throws rather than returning `[]` when
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
 * ANTI-VACUITY GUARD, the same one `reifyGrammarCorpus.test.ts` carries. Almost
 * every assertion here is `countErrorNodes(...) === 0`; if the helper ever
 * degraded to always returning 0 — a @lezer/lr change to `cursor.type.isError`,
 * or `tree.cursor()` gaining a default that skips error nodes — this whole file
 * would stay green while pinning nothing. `nodeNamesSpanning` needs no such
 * guard: it throws on an absent needle, and every use below asserts a non-empty
 * result.
 */
describe('reifyGrammarQualifiedRef — helper guards', () => {
  it('countErrorNodes still reports error nodes on input that cannot parse', () => {
    expect(countErrorNodes('@@@ !!! ???')).toBeGreaterThan(0);
  });

  it('countErrorNodes reports zero on input that parses', () => {
    expect(countErrorNodes('structure def Foo { }')).toBe(0);
  });
});

// ── Type position ───────────────────────────────────────────────────────────

describe('reify.grammar — qualified reference in TYPE position', () => {
  const src = 'structure def S { param p : pp.Pulley }';

  it('parses with zero error nodes', () => {
    expect(countErrorNodes(src)).toBe(0);
  });

  /**
   * `TypeExpr` wraps it, exactly as it wraps the `QualifiedType` and bare
   * `Identifier` arms — so the qualified form is reachable from all 20-odd
   * `TypeExpr` use sites at once, not bolted onto the `param` slot alone.
   */
  it('reduces `pp.Pulley` to a NamespacedName under TypeExpr', () => {
    expect(nodeNamesSpanning(src, 'pp.Pulley')).toEqual(['TypeExpr', 'NamespacedName']);
  });

  /** One node, not a self-nesting chain of them (#5957). */
  it('emits exactly one NamespacedName', () => {
    expect(countNodesNamed(src, 'NamespacedName')).toBe(1);
  });

  /**
   * `List<pp.Pulley>` falls out of the `TypeExpr` arm for free —
   * `TypeArgList → typeArg → TypeExpr` — so this is the pin that the arm went
   * on `TypeExpr` and not on `TypeAnnotation`.
   */
  it('nests inside a type argument list', () => {
    const nested = 'structure def S { param p : List<pp.Pulley> }';
    expect(countErrorNodes(nested)).toBe(0);
    expect(nodeNamesSpanning(nested, 'pp.Pulley')).toEqual([
      'TypeArgList',
      'TypeExpr',
      'NamespacedName',
    ]);
  });

  /**
   * NO qualified GENERIC form. `pp.Box<T>` is out of scope for μ and is
   * excluded from `namespaced_name` upstream too (the `namespaced_name` rule in
   * grammar.js is where the `optional(type_args)` formulation is recorded as
   * the one that produces an unresolved LR conflict). Pinned so a later
   * "obvious" widening is a deliberate choice rather than a silent one.
   */
  it('does NOT admit a qualified generic type', () => {
    expect(countErrorNodes('structure def S { param p : pp.Box<T> }')).toBeGreaterThan(0);
  });
});

// ── `sub` structure-name position ───────────────────────────────────────────

describe('reify.grammar — qualified reference in `sub` position', () => {
  /**
   * The specialization (`:`) arm. Its structure-name slot is NOT a `TypeExpr`
   * — it is the narrower `(ParameterizedType | Identifier)` — so widening
   * `TypeExpr` alone does not reach it, and this asserts the second edit.
   */
  it('parses the specialization arm and reduces to a NamespacedName', () => {
    const src = 'structure def S { sub p : pp.Pulley }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.Pulley')).toContain('NamespacedName');
    expect(countNodesNamed(src, 'NamespacedName')).toBe(1);
  });

  /** The instantiation (`=`) arm — the shape the prd-gate expr fixture uses. */
  it('parses the instantiation arm and keeps the argument list beside it', () => {
    const src = 'structure def S { sub p = pp.Pulley() }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.Pulley')).toContain('NamespacedName');
    expect(countNodesNamed(src, 'NamespacedName')).toBe(1);
    // The `()` stays the SubDeclaration's own `ArgList`; it must not be
    // swallowed into the qualified name or reduce the member to an expression.
    expect(nodeNamesSpanning(src, '()')).toEqual(['ArgList']);
  });

  it('parses named arguments in the instantiation arm', () => {
    const src = 'structure def S { sub p = pp.Pulley(bore: 5mm) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'bore: 5mm')).toContain('NamedArgument');
  });

  /**
   * THREE-SURFACE PARITY. The `:` arm's own type-argument tail applies to the
   * qualified structure name too: `sub h : pp.Pulley<T>`.
   *
   * MEASURED, not speculative — this is the one case where this grammar had
   * silently become STRICTER than the compiler. tree-sitter parses it with zero
   * ERROR nodes (its `sub_declaration` specialization (`:`) arm keeps
   * `optional(type_args)` after the widened `structure_name`) and `lower_sub`
   * builds a valid `SubDecl { structure_name: "pp.Pulley", type_args: [T] }`
   * with no diagnostic, while this grammar produced 2 error nodes until
   * `subSpecializedName` grew the matching tail. An editor that rejects what the
   * compiler accepts degrades silently — the exact failure this file's header
   * names.
   *
   * The node shape mirrors upstream's: `NamespacedName` and `TypeArgList` are
   * SIBLINGS, not a `ParameterizedType` wrapper, because the qualified name is
   * not itself parameterizable (see the next test).
   */
  it('accepts a type-argument tail on a qualified specialization name', () => {
    const src = 'structure def S { sub h : pp.Pulley<T> }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.Pulley')).toEqual(['NamespacedName']);
    expect(nodeNamesSpanning(src, 'T')).toEqual(['TypeArgList', 'TypeExpr', 'Identifier']);
    expect(countNodesNamed(src, 'NamespacedName')).toBe(1);
  });

  /**
   * The excluded companion, keeping the two forms distinguishable: the tail
   * belongs to the `sub` arm, NOT to `NamespacedName`. A qualified GENERIC in
   * TYPE position stays rejected — pinned again here, next to the acceptance
   * above, so neither can be widened by accident on the strength of the other.
   * (The same pair is pinned on tree-sitter by
   * `qualified_generic_in_type_position_still_errors`.)
   */
  it('still rejects a qualified generic in type position', () => {
    expect(countErrorNodes('structure def S { param p : pp.Box<T> }')).toBeGreaterThan(0);
  });

  /** The `=` arm's pre-existing tail is unaffected. */
  it('keeps the instantiation arm type-argument tail working', () => {
    const src = 'structure def S { sub p = pp.Pulley<T>() }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.Pulley')).toEqual(['NamespacedName']);
    expect(nodeNamesSpanning(src, '()')).toEqual(['ArgList']);
  });

  /** Negative control: the unqualified specialized form is unchanged. */
  it('leaves an unqualified specialized name a ParameterizedType', () => {
    const src = 'structure def S { sub h : Pulley<T> }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'Pulley<T>')).toEqual(['ParameterizedType']);
    expect(countNodesNamed(src, 'NamespacedName')).toBe(0);
  });
});

// ── Expression position — the qualified CALL form ───────────────────────────

describe('reify.grammar — qualified CALL in expression position', () => {
  /**
   * The callee stays a full `MemberAccess`, mirroring how `TraitMethodCall`
   * takes a whole `QualifiedAccess` — and mirroring the tree-sitter rule, whose
   * callee is a full `member_access` for the same reason. So the call-LESS
   * `pp.Pulley` and the callee of `pp.Pulley()` are the SAME node kind, and the
   * resolution phase (task ν) sees one uniform base.
   */
  it('reduces `pp.compute(1)` to a NamespacedCall over a MemberAccess callee', () => {
    const src = 'structure def S { let x = pp.compute(1) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.compute(1)')).toEqual(['NamespacedCall']);
    expect(nodeNamesSpanning(src, 'pp.compute')).toEqual(['MemberAccess']);
    expect(nodeNamesSpanning(src, '(1)')).toEqual(['ArgListExpr']);
    // One node, not a self-nesting chain of them (#5957).
    expect(countNodesNamed(src, 'NamespacedCall')).toBe(1);
  });

  it('parses a nullary qualified call', () => {
    const src = 'structure def S { let x = pp.Pulley() }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.Pulley()')).toEqual(['NamespacedCall']);
  });

  it('parses mixed positional and named arguments', () => {
    const src = 'structure def S { let x = pp.compute(1, scale: 2) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.compute(1, scale: 2)')).toEqual(['NamespacedCall']);
    expect(nodeNamesSpanning(src, 'scale: 2')).toContain('NamedArgument');
  });

  /**
   * The call-LESS form deliberately stays a plain `MemberAccess`: `pp.Pulley`
   * is indistinguishable from `self.width` at parse time, so the qualified-vs-
   * member disambiguation is deferred to resolution (task ν) on every surface,
   * this one included. A `NamespacedName` appearing here would be this grammar
   * claiming a distinction the other two do not make.
   */
  it('leaves a call-less dotted expression as a plain MemberAccess', () => {
    const src = 'structure def S { let x = pp.Pulley }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'pp.Pulley')).toEqual(['MemberAccess']);
    expect(countNodesNamed(src, 'NamespacedName')).toBe(0);
  });
});

// ── The two prd-gate fixtures, end to end ───────────────────────────────────

describe('reify.grammar — prd-gate qualified-reference fixtures', () => {
  for (const relPath of [
    'tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri',
    'tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri',
  ]) {
    it(`parses ${relPath} with zero error nodes`, () => {
      const src = readFileSync(join(REPO_ROOT, relPath), 'utf-8');
      expect(countErrorNodes(src)).toBe(0);
    });
  }
});

// ── Negative controls ───────────────────────────────────────────────────────
//
// All five are MEASURED clean today. They are here because the two new
// productions both open on `Identifier` and one of them competes with
// `MemberAccess` for the token after the dot — the two ways this change can
// break something are by capturing a shape that already had a home, and by
// disturbing the `::` family that spells qualification the other way. Each
// control therefore pins the NODE KIND, not just the error count: a shape that
// silently reduces to `NamespacedName` instead of `Identifier` still counts
// zero errors.

describe('reify.grammar — qualified-ref negative controls', () => {
  it('an unqualified type annotation stays a bare Identifier', () => {
    const src = 'structure def S { param p : Pulley }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'Pulley')).toEqual(['TypeExpr', 'Identifier']);
    expect(countNodesNamed(src, 'NamespacedName')).toBe(0);
  });

  it('a dotted member access stays a MemberAccess', () => {
    const src = 'structure def S { let x = obj.width }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'obj.width')).toEqual(['MemberAccess']);
    expect(countNodesNamed(src, 'NamespacedCall')).toBe(0);
  });

  it('an unqualified call stays a FunctionCall', () => {
    const src = 'structure def S { let x = plain(1) }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'plain(1)')).toEqual(['FunctionCall']);
    expect(countNodesNamed(src, 'NamespacedCall')).toBe(0);
  });

  it('`::` in type position stays a QualifiedType', () => {
    const src = 'structure def S { param p : Beam::Material }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'Beam::Material')).toEqual(['TypeExpr', 'QualifiedType']);
  });

  it('`::` in call position stays a TraitMethodCall', () => {
    const src = 'structure def S { let x = Foo::bar() }';
    expect(countErrorNodes(src)).toBe(0);
    expect(nodeNamesSpanning(src, 'Foo::bar()')).toEqual(['TraitMethodCall']);
    expect(nodeNamesSpanning(src, 'Foo::bar')).toEqual(['QualifiedAccess']);
  });
});


// ── Item-boundary trade (separator-less repeats) ────────────────────────────
//
// `namespaced_call`'s `prec(12)` (`NamespacedCall` here) makes the parser
// prefer SHIFT over reduce when an item that ends in `.name` is followed by an
// item that opens with `(`, in a body whose repeat has no separator. The two
// such bodies are `ConstraintDefinition`'s and `RelateBlock`'s; in both, the
// two source lines collapse into ONE item whose head is a qualified call.
//
// This is pinned, not merely tolerated. The hazard CLASS predates task 5495 μ:
// `function_call`'s prec 11 already joins a BARE `ident` with a following `(`
// the same way — the control below measures it — so μ WIDENS an accepted trade
// rather than inventing one. It is latent in practice: no committed `.ri`
// under examples/ or tests/prd-gate/fixtures/ has a line ending in `.ident`
// directly above a line opening with `(`. The trade itself is priced on the
// `itemStart`/`postfix` precedence block in reify.grammar.
//
// The tree-sitter twin of this block is
// tree-sitter-reify/test/corpus/namespaced_ref.txt's three "item boundary"
// cases — the same three sources, the same joined readings. Both surfaces must
// move together or the editor becomes a fourth dialect.

describe('reify.grammar — item boundary in separator-less repeats', () => {
  it('joins a predicate ending `.name` with the next parenthesised predicate', () => {
    const src = 'constraint def C {\n  a.b\n  (x) > 0\n}\n';
    expect(countErrorNodes(src)).toBe(0);
    // ONE predicate, not two — that IS the sacrifice.
    expect(countNodesNamed(src, 'ConstraintDefPredicate')).toBe(1);
    expect(nodeNamesSpanning(src, 'a.b\n  (x)')).toEqual(['NamespacedCall']);
  });

  it('CONTROL: a bare ident joins the same way, so the class predates 5495', () => {
    const src = 'constraint def C {\n  a\n  (x) > 0\n}\n';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'ConstraintDefPredicate')).toBe(1);
    expect(nodeNamesSpanning(src, 'a\n  (x)')).toEqual(['FunctionCall']);
    expect(countNodesNamed(src, 'NamespacedCall')).toBe(0);
  });

  it("joins the same way in relate's RelationMember repeat", () => {
    const src = 'structure def S {\n  relate {\n    a.b\n    (x)\n  }\n}\n';
    expect(countErrorNodes(src)).toBe(0);
    expect(countNodesNamed(src, 'RelationMember')).toBe(1);
    // `RelationMember` is span-identical here — the member IS the joined call.
    expect(nodeNamesSpanning(src, 'a.b\n    (x)')).toEqual(['RelationMember', 'NamespacedCall']);
  });
});
