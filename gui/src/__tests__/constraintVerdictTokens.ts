/**
 * The constraint-verdict wire-token extraction over gui/src-tauri/src/engine.rs
 * (task 6723, PRD-4 β).
 *
 * WHY THIS MODULE EXISTS — i.e. why the tokens are READ FROM RUST SOURCE
 * rather than hardcoded as lower-case literals in the consuming test.
 *
 * `ConstraintData.status` is produced in Rust and consumed in TypeScript, and
 * the two sides silently disagreed on its casing for a long time: the engine
 * emitted `"Satisfied"`/`"Violated"`/`"Indeterminate"` while every frontend
 * consumer (ConstraintPanel's STATUS_PRIORITY/statusIcon/statusTitle,
 * StatusBar's constraintSummary, ChatPanel's hasViolatedConstraints, and the
 * `[data-status="…"]` CSS selectors) compared against lower-case.  Both suites
 * were green throughout, because ~60 frontend fixtures hand-author the
 * lower-case token and the Rust tests pin the PascalCase one.  Each side
 * asserted only that it agreed with ITSELF.
 *
 * A hardcoded lower-case literal in the parity fixture would recreate exactly
 * that lying fixture.  So `constraintVerdictParity.test.ts` builds its payload
 * from whatever `extractVerdictTokens(readEngineSource())` returns — the
 * producer's real bytes drive the consumer, which is what makes the pin
 * two-way.  If somebody re-capitalises the Rust tokens, the extracted values
 * change and the frontend assertions red immediately.
 *
 * WHERE IT LIVES.  gui/tsconfig.json is `include: ["src"]`, so a module under
 * gui/src/__tests__/ is inside tsc's strict program.  Following the
 * ./toolDefNames.ts precedent, a non-suite helper lives here with no `.test.`
 * segment so vitest's default include does not collect it as a suite, and the
 * module is vitest-free: pure functions plus a plain read, with every `expect`
 * in the importing `.test.ts` files.
 *
 * The pure function takes a source STRING rather than doing its own I/O, which
 * is what lets ./constraintVerdictTokens.test.ts pin every match form from a
 * string literal with no on-disk fixture.
 */
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The three variants of `reify_constraints::Satisfaction`. */
export type SatisfactionVariant = 'Satisfied' | 'Violated' | 'Indeterminate';

/**
 * The token set is CLOSED at three (PRD §4.2 C2 forbids collapsing the
 * tri-state; `gui-on-demand-measurement.md` routes measured verdicts through
 * this same three-valued `Satisfaction` with measurement state as an
 * orthogonal axis, not a fourth token).
 */
export const SATISFACTION_VARIANTS: readonly SatisfactionVariant[] = [
  'Satisfied',
  'Violated',
  'Indeterminate',
];

/**
 * Extract the wire token each `Satisfaction` variant is serialised as.
 *
 * The pattern deliberately spans BOTH shapes engine.rs has worn:
 *
 *   - the two duplicated `match entry.satisfaction { … }` blocks that existed
 *     in `build_constraints` and `surface_geometry_derived_cells` before task
 *     6723, and
 *   - the single `satisfaction_token()` helper those two collapsed into.
 *
 * Both are `Satisfaction::Variant => "token"` arms, so the extraction survives
 * its own refactor unchanged.  The Indeterminate GUARD comparisons in
 * `surface_geometry_derived_cells` route through `satisfaction_token(
 * Satisfaction::Indeterminate)` — a call, not a match arm — so they are never
 * captured here and cannot contribute a phantom token.
 *
 * ANTI-RUBBER-STAMP.  An empty or partial extraction must be a LOUD failure,
 * never a silently-vacuous pass: a `Satisfaction` rename that stops matching
 * would otherwise hollow out the parity guard while leaving it green.  So this
 * throws when a variant is unmatched, and throws when one variant maps to two
 * different tokens (which is precisely the two-drifting-copies state this task
 * removed).
 */
export function extractVerdictTokens(rustSource: string): Map<SatisfactionVariant, string> {
  // Constructed per call so a stale `lastIndex` can never leak between callers.
  const arm = /Satisfaction::(Satisfied|Violated|Indeterminate)\s*=>\s*"([A-Za-z]+)"/g;

  const found = new Map<SatisfactionVariant, string>();
  for (const m of rustSource.matchAll(arm)) {
    const variant = m[1] as SatisfactionVariant;
    const token = m[2];
    const prior = found.get(variant);
    if (prior !== undefined && prior !== token) {
      throw new Error(
        `Ambiguous constraint-verdict extraction: Satisfaction::${variant} maps to both ` +
          `"${prior}" and "${token}". The wire token has exactly one source of truth ` +
          `(engine.rs's satisfaction_token); two disagreeing arms mean it has drifted again.`,
      );
    }
    found.set(variant, token);
  }

  const missing = SATISFACTION_VARIANTS.filter((v) => !found.has(v));
  if (missing.length > 0) {
    throw new Error(
      `Constraint-verdict extraction found no token for Satisfaction::` +
        `${missing.join(', Satisfaction::')} (matched ${found.size} of ` +
        `${SATISFACTION_VARIANTS.length}). Either engine.rs no longer emits the verdict ` +
        `tokens as \`Satisfaction::Variant => "token"\` match arms, or \`Satisfaction\` was ` +
        `renamed. Fix the pattern rather than letting the parity guard pass vacuously.`,
    );
  }

  return found;
}

/**
 * Read gui/src-tauri/src/engine.rs, the sole producer of `ConstraintData.status`.
 *
 * The `fileURLToPath` + `path.dirname` + `path.resolve` idiom is carried over
 * from ./toolDefNames.ts:133-136 (which documents why naive URL-relative `..`
 * math over-shoots). Segment count here:
 *
 *   <gui>/src/__tests__/constraintVerdictTokens.ts
 *              ^^^^^^^^   (dirname = __tests__/)
 *         ^^^               (..     = src/)
 *   ^^^^^                   (..     = gui/)
 */
export function readEngineSource(): string {
  const dir = path.dirname(fileURLToPath(import.meta.url));
  return readFileSync(path.resolve(dir, '..', '..', 'src-tauri', 'src', 'engine.rs'), 'utf-8');
}
