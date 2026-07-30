/**
 * The structural conventions the live `smoke_*.mjs` e2e drivers must follow —
 * today, the one that says a driver never re-implements the `open_file` retry
 * policy (task 5857).
 *
 * WHY A SOURCE-LEVEL CHECK IS THE ONLY SIGNAL. A driver needs a live reify-gui
 * (WebKit WebView + OCCT) to do anything at all, so CI can never execute one and
 * `node --check` catches syntax alone. The retry policy's own BEHAVIOUR is
 * covered where it lives — `openFileWithRetry` in `./smokeDriverGuards.mjs`,
 * pinned by `./smokeDriverGuards.test.ts`. What no test could reach is the
 * structural claim that every driver actually goes THROUGH it: an inline copy of
 * the loop is perfectly valid JavaScript that simply never gets the helper's
 * transport-error folding, and it fails only during a live run, if at all. This
 * is `./sharedModuleLoad.ts`'s pattern applied to the call-shape half of the
 * same seam.
 *
 * WHY IT MATCHES ON THE CALL SHAPE. `/\brpc\s*\(\s*["']open_file["']/` and not a
 * bare `open_file` substring, because the tool name legitimately appears in
 * PROSE COMMENTS in all six drivers and in a string literal that SURVIVES
 * migration — `log('Opening … via open_file (with retry for WebView init)…')` at
 * `./smoke_find_uses.mjs:103`. A substring match reports the two already-migrated
 * drivers as violators. Comments are stripped on top of that, because a comment
 * may legitimately show an example call; `./sharedModuleLoad.ts:64-68` documents
 * that same false positive and explicitly prescribes stripping over narrowing
 * the regex, which trades one blind spot for another.
 *
 * This module is vitest-free, like every other helper in this directory
 * (assertions.ts, diff.ts, paths.ts, rpc.ts, sharedModuleLoad.ts): it exposes a
 * pure predicate and plain reads, and every `expect` lives in
 * `./smokeDriverConventions.test.ts`. The predicate doing no I/O is what lets
 * each false-positive shape be pinned from a string literal with no on-disk
 * fixture.
 */
import * as fs from "node:fs";

import { VISUAL_DIR } from "./sharedModuleLoad.js";

/**
 * The stable identity of a violation. Assert on THIS, never on the message.
 *
 * `inline-open-file` — the driver calls the `open_file` tool directly instead of
 * going through `openFileWithRetry`, re-implementing the retry budget, the `.ok`
 * verdict and the failure wording that `./smokeDriverGuards.mjs` owns.
 */
export type SmokeDriverViolationCode = "inline-open-file";

/**
 * One convention a driver source breaks.
 *
 * The split exists so the two halves can evolve independently: `code` is the
 * contract the suite asserts on, leaving `message` free to be reworded or
 * enriched — with the offending line number, say — without churning a single
 * assertion.
 */
export interface SmokeDriverViolation {
  readonly code: SmokeDriverViolationCode;
  readonly message: string;
}

/**
 * `source` with `//`-to-EOL and `/* … *\/` spans blanked out.
 *
 * REQUIRED, not defensive. Without it the block comment
 * `/* e.g. await rpc('open_file', {path}) *\/` — a perfectly reasonable thing to
 * write next to the helper — reads as a real call site. Newlines inside a block
 * comment are preserved so a future message can still report a line number
 * against the ORIGINAL source.
 *
 * Not a parser: a `//` sequence inside a string literal (a URL, say) blanks the
 * rest of that line. Accepted, because the consequence is a MISSED violation in
 * a line that would have to contain both a URL and an inline `open_file` call,
 * never a false alarm on a compliant driver — and a false alarm is the failure
 * mode that would get "fixed" by editing correct code.
 */
export function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, (span) => span.replace(/[^\n]/g, " "))
    .replace(/\/\/[^\n]*/g, "");
}

/** A direct call of the `open_file` tool, in either quote style, however spaced. */
const INLINE_OPEN_FILE = /\brpc\s*\(\s*["']open_file["']/;

/**
 * Every convention `source` breaks, as a driver in this directory.
 *
 * Returns a list; `[]` means compliant. A LIST rather than a boolean on purpose:
 * the assertion failure then names which convention broke, instead of dumping
 * the whole source blob.
 *
 * Reports each convention AT MOST ONCE — a driver carrying the full inline retry
 * loop has one problem, not one per line that mentions the tool.
 */
export function findSmokeDriverConventionViolations(source: string): SmokeDriverViolation[] {
  const violations: SmokeDriverViolation[] = [];
  const code = stripComments(source);
  if (INLINE_OPEN_FILE.test(code)) {
    violations.push({
      code: "inline-open-file",
      message:
        "calls `open_file` directly instead of `openFileWithRetry` from ./smokeDriverGuards.mjs",
    });
  }
  return violations;
}

/**
 * The live `smoke_*.mjs` e2e drivers this directory holds.
 *
 * Enumerated, not discovered: this is the table `./smokeDriverConventions.test.ts`
 * drives its `it.each` off, and a directory read there would let a discovery bug
 * collapse it to zero registered tests — trading a loud gap for a silent one
 * (`./sharedModuleLoad.ts:106-110`). {@link discoverSmokeDrivers} cross-checks it.
 */
export const SMOKE_DRIVERS = [
  "smoke_appearance_e2e.mjs",
  "smoke_diagnostics_e2e.mjs",
  "smoke_find_uses.mjs",
  "smoke_mesh_count_parity_e2e.mjs",
  "smoke_multi_pane_e2e.mjs",
  "smoke_surface_finish_viewport_e2e.mjs",
];

/**
 * Every `.mjs` in `dir` that IS a `smoke_*` driver — the exact complement of
 * `discoverSharedEsmModules`'s filter, so the two tables together cover every
 * `.mjs` in the directory and neither can silently drop one.
 *
 * Used ONLY by the completeness guard, never as the `it.each` source; see
 * {@link SMOKE_DRIVERS}. The `dir` parameter exists so the filter rule stays
 * pinnable against a fixture directory in isolation.
 */
export function discoverSmokeDrivers(dir: string = VISUAL_DIR): string[] {
  return fs.readdirSync(dir).filter((name) => name.endsWith(".mjs") && name.startsWith("smoke_"));
}
