/**
 * The constraint-verdict wire-token extraction over gui/src-tauri/src/engine.rs
 * (task 6723, PRD-4 β). The contract these tokens obey is canonical on
 * `ConstraintData.status` in gui/src-tauri/src/types.rs.
 *
 * WHY THE TOKENS ARE READ FROM RUST SOURCE rather than hardcoded as lower-case
 * literals in the consuming test: ~60 existing frontend fixtures hand-author the
 * lower-case token, which is exactly why the vitest suite stayed green for as
 * long as the engine emitted PascalCase — each side only ever asserted that it
 * agreed with ITSELF. A hardcoded literal here would faithfully recreate that
 * lying fixture, so ./constraintVerdictParity.test.ts builds its payload from
 * whatever `extractVerdictTokens(readEngineSource())` returns: the producer's
 * real bytes drive the consumer, which is what makes the pin two-way.
 *
 * SCAFFOLDING, NOT PERMANENT ARCHITECTURE. This module exists only because
 * `status` crosses the wire as an untyped `String` produced by a hand-written
 * mapping. A serde-derived `ConstraintStatus` enum (`#[serde(rename_all =
 * "lowercase")]`, `From<Satisfaction>`) plus the matching TypeScript union would
 * make the casing DERIVED rather than hand-written, and re-introducing the drift
 * a compile error. When that lands, this module, ./constraintVerdictTokens.test.ts
 * and the source-reading half of ./constraintVerdictParity.test.ts should be
 * DELETED, not maintained. The narrowing is filed as follow-up work (see
 * `ConstraintData.status` in ../types.ts).
 *
 * WHERE IT LIVES. gui/tsconfig.json is `include: ["src"]`, so a module under
 * gui/src/__tests__/ is inside tsc's strict program. Following the
 * ./toolDefNames.ts precedent it has no `.test.` segment (vitest's default
 * include does not collect it as a suite) and is vitest-free: pure functions
 * plus a plain read, with every `expect` in the importing `.test.ts`. Taking a
 * source STRING rather than doing its own I/O is what lets the unit suite pin
 * every match form from a string literal, with no on-disk fixture.
 */
import { readTauriSrc } from './tauriSource';

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
 * The pattern matches `Satisfaction::Variant => "token"` match arms wherever
 * they appear, so it is indifferent to how many functions carry them and
 * survives a refactor of the producer unchanged; arms that AGREE collapse to one
 * entry per variant.  The Indeterminate GUARD comparisons in
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
 * Shares its path computation with `readDebugServerSource` (./toolDefNames.ts)
 * via `readTauriSrc`.
 */
export function readEngineSource(): string {
  return readTauriSrc('engine.rs');
}
