import { describe, it, expect } from 'vitest';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { countErrorNodes } from './lezerProbeHelpers';
import EXPECTED_CLEAN from './reifyGrammarCorpus.expected-clean.json';

/**
 * Corpus drift ledger for the GUI Lezer grammar
 * (`gui/src/editor/reify.grammar`): walks every committed `.ri` under the
 * corpus roots and asserts that each path pinned in
 * `reifyGrammarCorpus.expected-clean.json` still parses with zero error nodes.
 * The per-production shape tests live in `reifyGrammarCorpus.test.ts`.
 */

/** Repo root, three levels up from `gui/src/__tests__/`. */
const REPO_ROOT = join(__dirname, '../../..');

function readFixture(relPath: string): string {
  return readFileSync(join(REPO_ROOT, relPath), 'utf-8');
}

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
// pinned set (now reifyGrammarCorpus.expected-clean.json), taking it from 90 to
// 301 of the same 329 files:
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
// and is guarded by explicit non-regression assertions in
// reifyGrammarCorpus.test.ts rather than by any movement here.
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
// node-name assertions in their slices in reifyGrammarCorpus.test.ts rather
// than by any movement here.
// For two of them that guarding is the ONLY guarding there is, because the
// wrong reading parses clean: an argument-position construction could have been
// widened into `primaryExpression` and silently swallowed a `match`/`where`/
// `connect` body, and a `sub_relate_block` resolved with precedence instead of
// the `~poseWhere` marker would silently have stolen every post-pose `where`
// from `GuardedBlock`. Neither would have shown up as an error node.
//
// FOUR OF THE SIX `constraint def` FILES DID NOT TURN CLEAN, and their absence
// from reifyGrammarCorpus.expected-clean.json is not a regression. Each keeps
// a second, out-of-scope blocker — in order: `some(u)`, `meta.material`,
// `meta.project`, and a member-level `@annotation`. Their error counts did
// drop (integration_corner_cases 9→7, integration_full_v01 14→5,
// m11_annotations 18→10, m9_combined 9→4), which is real progress that this
// pinned set is deliberately unable to express.
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
// by tests in reifyGrammarCorpus.test.ts so the next round does not
// re-litigate them:
//
//   - "two stackup files" was not a list-literal gap. It was `chain`, which
//     round 2 had promoted to a hard `kw<>`; that is context-free in Lezer, so
//     `chain` stopped being a legal identifier in three committed files. A
//     self-inflicted regression, and the cheapest fix in the round.
//   - "three remaining one-offs" was two. Chained comparison `a < b < c < d`
//     already parsed with zero errors in both binding and constraint position;
//     integration_corner_cases.ri:130 is `let chain = …`, so its errors were
//     the keyword above and :132-134 were cascade.
//   - "arrow (function) types in a param annotation" was already reachable —
//     `FunctionType` has been in the grammar all along.
//
// WHY THE LAST THREE FILES REMAIN — one line each, because a bare count is
// not a finding and "which three, and why" is. Named in KNOWN_NOT_CLEAN
// below, which the ledger checks rather than trusts:
//
//   - arrow_type.ri — NOT A GRAMMAR GAP AT ALL. Its `param` is at TOP LEVEL,
//     and `param` is not a top-level declaration in tree-sitter's
//     `_declaration` (grammar.js:134) or in the compiler's `lower_source_file`
//     (crates/reify-syntax/src/ts_parser.rs:299). Admitting it here would make
//     the editor accept what neither of those does — a third dialect. The
//     fixture's own header declares it a deliberate FAIL probe. Both halves are
//     pinned in reifyGrammarCorpus.test.ts: the member-position arrow type IS
//     clean, and the top-level `param` MUST error.
//   - shear_angles_vec3_angle_param_pre.ri and shear_angles_vec3_wrongq_ctrl.ri
//     — both error solely on the local binding name `out`: `let out =
//     f(axis)`. Measured: `let out = 1` inside a `structure def` block yields
//     2 error nodes where `let q = 1` yields 0, and `param out : Length =
//     1mm` also yields 2 — the Vector3<Angle> parameter beside it is not
//     implicated (`fn f(v : Vector3<Angle>) -> Real { 1.0 }` parses clean).
//     `out` is a hard `kw<>`, reserved for the `port out` direction form —
//     the same self-inflicted class as round 4's `chain`/`meta` demotions to
//     `ekw<>`. Grammar CAPABILITY, owned by the #5907 series, not coverage;
//     both stay unpinned here.
//
// THE ARITHMETIC ABOVE IS INHERITED, AND EVERY DENOMINATOR IN IT IS STALE BY
// CONSTRUCTION — each was a snapshot of the corpus on the day its round ran,
// and the corpus keeps growing every round after. #6605 re-measured through
// the ledger's own walk and closed the gap those stale snapshots kept
// finding: the pinned set (reifyGrammarCorpus.expected-clean.json) IS the
// full measured-clean set as of this commit. The LIVE arithmetic is printed
// by this test's own failure message (`measured N clean of M …; K clean files
// are not pinned`) — read it off a run, never off this or any other comment.
//
// AND NOTE WHAT THIS BLOCK DELIBERATELY DOES NOT CONTAIN. The per-production
// reasoning for every change above — why `ekw<>` and not `kw<>`, why the
// callee and not the `ReservedWord` list, why `·` cannot escape `unitOp` —
// lives on the productions themselves in reify.grammar. #5941 was amended
// TWICE to stop reifyGrammarCorpus.test.ts restating them (`eb2394f2cd`,
// `05e5c50eac`); read the grammar, not a second copy that drifts.
//
// `variant_construction` left round 3's inherited list before round 4 began:
// argument position is now admitted.
// The narrowing that remains is `primaryExpression` only, it is deliberate, and
// the note on that production in reify.grammar prices it — see also the pin
// in reifyGrammarCorpus.test.ts asserting that expression-position `Name { … }`
// must STILL error.
//
// AND ONE KNOWN DIVERGENCE, which is not a gap and must not be "fixed" blindly.
// Underscore-separated DECIMAL literals did not get their own node: `1_000_000`
// still lexes as `QuantityLiteral` (`1` + unit `_000_000`). The companion-token
// trick that made `0xDEAD_BEEF` one `RadixLiteral` cannot be reused, because
// lezer token precedence beats longest match: a separator-aware `Number`
// ordered above `QuantityLiteral` takes `1_000` out of the ATTESTED `1_000mm`
// (examples/numeric_separators.ri:17) and strands the `mm`, turning a
// committed line into an error node. Zero ledger impact either way; pinned by
// shape tests in reifyGrammarCorpus.test.ts so it stays visible. See the
// fuller accounting on the `Number` token in reify.grammar, which prices the
// full trade.
//
// WHY A PINNED SET AND NOT A COUNT. An absolute floor (`clean.length >= 90`)
// is coupled to corpus INVENTORY, not to grammar capability: deleting or
// renaming a currently-clean `.ri` — a routine refactor with no grammar
// involvement — would fail with "the grammar regressed", a false accusation
// whose obvious fix (lower the floor) also masks any genuine regression
// landing alongside it. Conversely, adding 50 unparseable examples would keep
// a count green while coverage rotted. So the ledger pins the SET instead:
// every path in reifyGrammarCorpus.expected-clean.json is expected to parse
// clean, filtered by what still exists on disk. Removals drop out naturally, a
// genuine regression names the exact file that stopped parsing, and a coverage
// gain is a visible one-line addition to that file. Whether every clean file
// is pinned is measured and reported in the failure message below
// (`unpinnedClean`) but deliberately not gated — gating it would force every
// future clean prd-gate fixture into a matching _GUI_COUPLED_RI_FIXTURES edit
// with no grammar change behind it. The three files named above ARE gated:
// KNOWN_NOT_CLEAN below asserts each still fails to parse, so a capability
// gain reads as a failure instead of a stale exclusion.
//
// #6605: coverage, not capability. No production in reify.grammar changed;
// every path pinned by this round already parsed clean and was simply
// unlisted.

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

// The three files named in "WHY THE LAST THREE FILES REMAIN" above — checked
// below against the live not-clean set instead of only asserted in prose, so
// a grammar change that starts parsing one of them clean fails loudly rather
// than leaving a stale exclusion.
const KNOWN_NOT_CLEAN = [
  'tests/prd-gate/fixtures/arrow_type.ri',
  'tests/prd-gate/fixtures/shear_angles_vec3_angle_param_pre.ri',
  'tests/prd-gate/fixtures/shear_angles_vec3_wrongq_ctrl.ri',
];

// Wall-clock for the ledger below. The walk itself is fast — 329 files parse
// in ~0.75 s when this file runs alone — but under the full suite (159 test
// files across parallel workers) it is CPU-starved and overran vitest's 15 s
// default, so the timeout is raised explicitly rather than left to chance.
const LEDGER_TIMEOUT_MS = 120_000;

describe('reify.grammar — corpus drift ledger', () => {
  it('still parses every committed .ri that is expected to parse clean', () => {
    // The walk and all ~329 parses live INSIDE the `it`, not in the describe
    // body: a throw at collection time (a fixture removed mid-walk, an
    // unreadable file, a pathological input) would otherwise fail this whole
    // file at collection with an error that does not even name the offending
    // path.
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

    // The converse of `regressed`: KNOWN_NOT_CLEAN names files this ledger
    // currently expects to fail, so if the grammar gains the capability to
    // parse one of them, that must fail loudly here rather than leave "WHY
    // THE LAST THREE FILES REMAIN" above silently wrong.
    const knownNotCleanPresent = KNOWN_NOT_CLEAN.filter((p) => existsSync(join(REPO_ROOT, p)));
    const capabilityGained = knownNotCleanPresent.filter((p) => cleanSet.has(p));

    expect(
      capabilityGained,
      `These files are listed in KNOWN_NOT_CLEAN ("WHY THE LAST THREE FILES ` +
        `REMAIN" above) but now parse with zero error nodes — move each into ` +
        `EXPECTED_CLEAN and drop it from KNOWN_NOT_CLEAN:\n` +
        `${capabilityGained.map((p) => `  ${p}`).join('\n')}`,
    ).toEqual([]);
  }, LEDGER_TIMEOUT_MS);
});
