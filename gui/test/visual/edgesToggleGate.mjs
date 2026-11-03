/**
 * Every decision the live edges-in-black toggle smoke
 * (`./smoke_edges_toggle_e2e.mjs`, task 9731, PRD B6) makes, as pure data: did
 * the toggle read true after turning it on and false after turning it off, and
 * did the design viewport actually change between the two screenshots. Each
 * check returns failure records whose `code` is one of
 * {@link EDGES_GATE_CODES}; {@link formatFailures} is the single place one
 * becomes English.
 *
 * PLAIN ESM, free of builtin-module imports: both the vitest suite
 * (`./edgesToggleGate.test.ts`) and the live driver (run with bare `node`) load
 * it. Image data is read through Uint8Array, never Buffer.
 */

export const EDGES_GATE_CODES = Object.freeze({
  /** store_state did not read viewport.edges === true after toggling on. */
  EDGES_NOT_ON: "edges-not-on-after-toggle",
  /** store_state did not read viewport.edges === false after toggling back off. */
  EDGES_NOT_OFF: "edges-not-off-after-toggle",
  /** The two screenshots have different dimensions, so they cannot be compared. */
  SHOT_SIZE_MISMATCH: "screenshot-size-mismatch",
  /** The edges-on screenshot barely differs from the edges-off one. */
  SHOTS_TOO_SIMILAR: "screenshots-too-similar",
});

/**
 * Black 1-px crease and silhouette lines around even one m5_geometry body in
 * the design viewport change thousands of pixels, while two renders of an
 * unchanged scene are pixel-identical — so 100 separates the overlay's signal
 * from a stray pixel.
 */
export const MIN_CHANGED_PIXELS = 100;

/**
 * Number of pixels that differ in any RGBA channel, or null when the two images
 * have different dimensions.
 *
 * @param {{width: number, height: number, data: Uint8Array}} a
 * @param {{width: number, height: number, data: Uint8Array}} b
 * @returns {number | null}
 */
export function countChangedPixels(a, b) {
  if (a.width !== b.width || a.height !== b.height) return null;
  let changed = 0;
  for (let i = 0; i < a.data.length; i += 4) {
    if (
      a.data[i] !== b.data[i] ||
      a.data[i + 1] !== b.data[i + 1] ||
      a.data[i + 2] !== b.data[i + 2] ||
      a.data[i + 3] !== b.data[i + 3]
    ) {
      changed++;
    }
  }
  return changed;
}

const sizeOf = (shot) => ({ width: shot.width, height: shot.height });

/**
 * Grade one toggle round trip: off-shot, toggle on, read state, on-shot, toggle
 * off, read state.
 *
 * @param {{
 *   edgesAfterOn: unknown,
 *   edgesAfterOff: unknown,
 *   offShot: {width: number, height: number, data: Uint8Array},
 *   onShot: {width: number, height: number, data: Uint8Array},
 * }} args
 * @returns {Array<{code: string} & Record<string, unknown>>}
 */
export function gradeEdgesToggle({ edgesAfterOn, edgesAfterOff, offShot, onShot }) {
  const failures = [];
  if (edgesAfterOn !== true) {
    failures.push({ code: EDGES_GATE_CODES.EDGES_NOT_ON, observed: edgesAfterOn });
  }
  if (edgesAfterOff !== false) {
    failures.push({ code: EDGES_GATE_CODES.EDGES_NOT_OFF, observed: edgesAfterOff });
  }
  const changedPixels = countChangedPixels(offShot, onShot);
  if (changedPixels === null) {
    failures.push({
      code: EDGES_GATE_CODES.SHOT_SIZE_MISMATCH,
      offSize: sizeOf(offShot),
      onSize: sizeOf(onShot),
    });
  } else if (changedPixels < MIN_CHANGED_PIXELS) {
    failures.push({
      code: EDGES_GATE_CODES.SHOTS_TOO_SIMILAR,
      changedPixels,
      minChangedPixels: MIN_CHANGED_PIXELS,
    });
  }
  return failures;
}

const size = ({ width, height }) => `${width}x${height}`;

const DESCRIBE = {
  [EDGES_GATE_CODES.EDGES_NOT_ON]: (f) =>
    `store_state viewport.edges read ${JSON.stringify(f.observed)} after toggling on (want true)`,
  [EDGES_GATE_CODES.EDGES_NOT_OFF]: (f) =>
    `store_state viewport.edges read ${JSON.stringify(f.observed)} after toggling off (want false)`,
  [EDGES_GATE_CODES.SHOT_SIZE_MISMATCH]: (f) =>
    `edges-off screenshot is ${size(f.offSize)} but edges-on is ${size(f.onSize)}`,
  [EDGES_GATE_CODES.SHOTS_TOO_SIMILAR]: (f) =>
    `only ${f.changedPixels} pixels changed between edges off and on (want >= ${f.minChangedPixels})`,
};

/**
 * One English line per failure, each led by its code.
 *
 * @param {Array<{code: string}>} failures
 * @returns {string}
 */
export function formatFailures(failures) {
  return failures
    .map((f) => `[${f.code}] ${DESCRIBE[f.code]?.(f) ?? JSON.stringify(f)}`)
    .join("\n");
}
