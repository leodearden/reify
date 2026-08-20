/**
 * The structural conventions the live `smoke_*.mjs` e2e drivers must follow —
 * today, two of them: a driver never re-implements the `open_file` retry policy
 * (task 5857), and a driver never reads the post-open `store_state` payload it
 * binds as `storeAfterOpen` without first diagnosing it (task 5884).
 *
 * THAT SECOND WORDING IS DELIBERATELY NARROWER than the rule a reader would
 * state from the rationale below ("never read a post-open `store_state` payload
 * undiagnosed"), because the narrow one is what actually ships: the check keys
 * on the conventional `storeAfterOpen` binding and its `?.editor` read, and sees
 * no other spelling of either. `STORE_STATE_READ` and `STORE_STATE_DIAG` below
 * each state what they leave unseen and why that gap errs toward a missed
 * violation; the header states the shipped rule so the two cannot drift.
 *
 * WHY THE SECOND ONE IS A CONVENTION AND NOT A CODE REVIEW NOTE. An in-band
 * `{error: '<msg>'}` envelope is TRUTHY, so the `!storeAfterOpen?.editor?.…`
 * guard every driver writes reads straight past a `store_state` outage to
 * `undefined` and reports `got: undefined` — a frontend-shaped verdict for a
 * tool-shaped failure, which is the most expensive kind of misattribution a
 * smoke driver can produce. `describeRpcFailure` in `./smokeDriverGuards.mjs`
 * folds that envelope into a named failure first. Four drivers wrote the same
 * unguarded chain independently (tasks 5827, 5883, 5884), so it is a shape the
 * next driver will reach for too.
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
import { VISUAL_DIR, partitionVisualMjs } from "./sharedModuleLoad.js";

/**
 * The stable identity of a violation. Assert on THIS, never on the message.
 *
 * `inline-open-file` — the driver calls the `open_file` tool directly instead of
 * going through `openFileWithRetry`, re-implementing the retry budget, the `.ok`
 * verdict and the failure wording that `./smokeDriverGuards.mjs` owns.
 *
 * `undiagnosed-store-state` — the driver reads `storeAfterOpen?.editor` without
 * first putting the payload through `describeRpcFailure`, so a truthy in-band
 * `{error: '<msg>'}` survives the `!x` guard, the chain reports
 * `activeFile: undefined`, and the run blames the frontend for what was a
 * `store_state` outage. Keyed on that exact binding and field, not on the
 * payload shape: another spelling of either is not seen at all (see
 * `STORE_STATE_READ`).
 */
export type SmokeDriverViolationCode = "inline-open-file" | "undiagnosed-store-state";

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
 * against the ORIGINAL source; only line NUMBERS survive, not columns, since the
 * `//` pass deletes rather than blanks.
 *
 * NOT A PARSER, and both known blind spots point the same way — toward a MISSED
 * violation, never a false alarm on a compliant driver. That asymmetry is the
 * whole acceptance argument: a missed violation leaves a driver un-migrated
 * until someone reads it, whereas a false alarm gets "fixed" by editing correct
 * code, which is strictly worse.
 *   - a `//` sequence inside a string literal (a URL, say) blanks the rest of
 *     that line, so a line carrying both a URL and an inline call goes unseen.
 *   - the block pass runs FIRST, so a `//` comment that happens to contain `/*`
 *     ("// see /* the helper") opens a block span that runs on to the next
 *     `*\/` anywhere below, blanking the real code in between.
 * Both are pinned by name in `./smokeDriverConventions.test.ts` rather than left
 * as prose, so a later "fix" that reverses the asymmetry fails loudly.
 */
export function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, (span) => span.replace(/[^\n]/g, " "))
    .replace(/\/\/[^\n]*/g, "");
}

/** A direct call of the `open_file` tool, in either quote style, however spaced. */
const INLINE_OPEN_FILE = /\brpc\s*\(\s*["']open_file["']/;

/**
 * The OPTIONAL-CHAIN read of the post-open `store_state` payload, however spaced.
 *
 * The optional chain and NOT a plain-dot `storeAfterOpen.editor` on purpose: the
 * defect class this convention defends against is SILENCE. `?.` reads past a
 * truthy `{error}` to `undefined`; the plain-dot form yields `undefined` at
 * `.editor` and then THROWS at `.activeFile`, which is a stack trace naming the
 * exact line — already loud, already attributable. Pinned by name as a
 * deliberate non-violation in `./smokeDriverConventions.test.ts`.
 *
 * COUPLED TO ONE SPELLING, deliberately and visibly: the conventional
 * `storeAfterOpen` binding and its `?.editor` field, NOT the payload shape. A
 * driver binding `postOpenStore`, or reading `storeAfterOpen?.viewports`, walks
 * straight past this — and such a read is already in the corpus, in
 * `./smoke_multi_pane_e2e.mjs`'s second `storeState` payload, which is diagnosed
 * today by author discipline rather than by this check. Widening to `store\w*`
 * would have to widen {@link STORE_STATE_DIAG} in lockstep, or every alternate
 * binding that IS diagnosed becomes a false alarm — the one direction this
 * module refuses to err in — so the narrow form ships and the gap is stated
 * rather than papered over. Like `stripComments`' blind spots it errs toward a
 * MISSED violation, and like them it is pinned by name in
 * `./smokeDriverConventions.test.ts` rather than left as prose.
 */
const STORE_STATE_READ = /\bstoreAfterOpen\s*\?\.\s*editor\b/;

/**
 * The diagnosis that must accompany that read, in whatever argument spelling.
 *
 * A WHOLE-FILE PRESENCE check, with no pairing to the read it guards: one
 * `describeRpcFailure(storeAfterOpen, …)` anywhere in the source satisfies it,
 * including one sitting BELOW the read, one inside a different function, and one
 * covering only the first of two `storeAfterOpen` bindings. Position is not
 * enforced — pairing it would mean parsing, and this module is a regex pass by
 * design. That gap errs the accepted way (a missed violation, never a false
 * alarm on a compliant driver) and is pinned by name alongside the others.
 */
const STORE_STATE_DIAG = /\bdescribeRpcFailure\s*\(\s*storeAfterOpen\b/;

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
  if (
    STORE_STATE_READ.test(code) &&
    // RAW `source`, deliberately, NOT the stripped `code` — and this is the line
    // a later "make it consistent" edit would change. `stripComments`' asymmetry
    // (see its docblock) holds only for a POSITIVE-presence match: a blind spot
    // blanks a real call site and the violation goes unseen. A NEGATIVE-presence
    // half INVERTS it — blanking the real `describeRpcFailure(storeAfterOpen, …)`
    // line makes `lacks DIAG` true and false-alarms a COMPLIANT driver, the
    // direction this module refuses to err in. Matching DIAG on the raw source
    // puts both blind spots out of reach here; the price is that a diagnosis
    // written only inside a comment suppresses the flag, which is a missed
    // violation — the direction already accepted. One rule for both halves: each
    // is matched against whichever text errs toward silence. Pinned in both
    // directions by ./smokeDriverConventions.test.ts.
    !STORE_STATE_DIAG.test(source)
  ) {
    violations.push({
      code: "undiagnosed-store-state",
      message:
        "reads `storeAfterOpen.editor` without first diagnosing the `store_state` RPC " +
        "via `describeRpcFailure` from ./smokeDriverGuards.mjs",
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
 * Every `.mjs` in `dir` that IS a `smoke_*` driver — the driver half of
 * `./sharedModuleLoad.ts`'s {@link partitionVisualMjs} split, whose docblock is
 * the one home of the rule and the reasoning behind it.
 *
 * Used ONLY by the completeness guard, never as the `it.each` source; see
 * {@link SMOKE_DRIVERS}. The `dir` parameter exists so this delegation stays
 * pinnable against a fixture directory in isolation.
 */
export function discoverSmokeDrivers(dir: string = VISUAL_DIR): string[] {
  return partitionVisualMjs(dir).drivers;
}
