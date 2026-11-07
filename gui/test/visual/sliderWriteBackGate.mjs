/**
 * The decision function behind the slider/edit-box write-back live gate
 * (task 7680; PRD docs/prds/v0_6/ai-native-editing.md, INV-GUI-3).
 *
 * THE SCENARIO. A user drags the MechanismPanel slider bound to
 * `SliderWriteBack.width`, then types into its PropertyEditor edit box, and the
 * `.ri` file on disk must follow:
 *
 *   - while the slider is HELD, only previews flow: the engine moves, the disk
 *     does not;
 *   - on RELEASE, exactly the default literal's span is rewritten;
 *   - an Export afterwards carries the released value, not the default;
 *   - an edit-box commit (Enter, then blur) rewrites the same span again.
 *
 * FAILURES ARE DATA: every predicate returns a list of `{code, ...observed}`
 * records whose `code` is one of {@link SLIDER_GATE_CODES}, and
 * {@link formatFailures} is the single place one becomes English.
 *
 * PLAIN ESM, free of builtin-module imports: both the vitest suite
 * (`./sliderWriteBackGate.test.ts`) and the live driver
 * (`./smoke_slider_write_back_e2e.mjs`, run with bare `node`) load it. Binary
 * data is read through Uint8Array/DataView, never Buffer.
 */

// ─── The subject ─────────────────────────────────────────────────────────────

/** Where the tracked fixture lives, relative to the repo root. */
export const FIXTURE_RELPATH = "gui/test/fixtures/slider_write_back.ri";

/** The live driver edits a `mkdtemp` COPY under this same basename. */
export const SUBJECT_BASENAME = "slider_write_back.ri";

const ENTITY = "SliderWriteBack";
const PARAM_NAME = "width";

/** The param under test: `<Entity>.<member>`, the engine_state / PropertyEditor id. */
export const CELL_ID = `${ENTITY}.${PARAM_NAME}`;

/** The descriptor cell of the one-body mechanism whose joint the param drives. */
export const MECHANISM_CELL_ID = `${ENTITY}.m1`;

/** The bound joint's index within that mechanism. */
export const JOINT_INDEX = 0;

/** The param's literal in the committed fixture. */
export const DEFAULT_LITERAL = "40mm";

export const JOINT_RANGE_MIN_MM = 0;
export const JOINT_RANGE_MAX_MM = 200;

/** The joint declaration, whose range sets the slider's min/max (step 1 mm). */
export const JOINT_RANGE_DECL = `prismatic(vec3(1, 0, 0), ${JOINT_RANGE_MIN_MM}mm .. ${JOINT_RANGE_MAX_MM}mm)`;

/** Slider values are the control's own `.value` strings: integer display mm. */
export const SLIDER_HOLD_FRAMES = Object.freeze(["50", "60", "70", "80"]);
export const SLIDER_HOLD_VALUE = "90";
export const SLIDER_RELEASE_VALUE = "120";

/** Edit-box keystrokes, ending on a literal the editor accepts. */
export const EDIT_ENTER_FRAMES = Object.freeze(["1", "15", "150", "150m"]);
export const EDIT_ENTER_VALUE = "150mm";
export const EDIT_BLUR_VALUE = "75mm";

/**
 * Millimetre tolerance on an engine reading: `si_value` crosses the wire in
 * metres and is scaled by 1000, so `===` would red on f64 round-trip noise. Same
 * basis as railLengtheningGate's RAIL_GATE_LENGTH_TOLERANCE_MM.
 */
export const LENGTH_TOLERANCE_MM = 1e-6;

/**
 * Millimetre tolerance on an exported extent: STL stores float32, whose ulp at
 * ≤ 200 is ≈ 1.5e-5 mm, and a box tessellation puts its vertices exactly on its
 * corners.
 */
export const STL_EXTENT_TOLERANCE_MM = 1e-3;

export const SLIDER_GATE_CODES = Object.freeze({
  /** The text holds no declaration with the literal the edit starts from. */
  declarationAbsent: "declaration-absent",
  /** The text holds it more than once, so a splice could not be exact. */
  declarationAmbiguous: "declaration-ambiguous",
  /** A commit left the disk byte-identical. */
  commitNotWritten: "commit-not-written",
  /** The disk changed, but not by exactly the one-span splice. */
  unexpectedDiff: "unexpected-diff",
  /** A held (uncommitted) gesture changed the disk. */
  previewWroteDisk: "preview-wrote-disk",
  /** A held gesture's value never reached the engine: the hold proved nothing. */
  previewNotLanded: "preview-not-landed",
  /** The export is not a binary STL (length ≠ 84 + 50·n). */
  stlMalformed: "stl-malformed",
  /** The export holds no triangles. */
  stlEmpty: "stl-empty",
  /** The exported X extent is not the expected length. */
  exportExtentMismatch: "export-extent-mismatch",
});

// ─── The source text ─────────────────────────────────────────────────────────

/** The param's declaration line (unindented) carrying `literal` as its default. */
export function paramDeclaration(literal) {
  return `param ${PARAM_NAME}: Length = ${literal}`;
}

/**
 * `before` with the ONE declaration defaulting to `fromLiteral` rewritten to
 * `toLiteral`, or the failure record saying why that splice is not exact.
 *
 * @param {string} before
 * @param {string} fromLiteral
 * @param {string} toLiteral
 * @returns {string | {code: string, declaration: string, count?: number}}
 */
export function expectedSourceAfterCommit(before, fromLiteral, toLiteral) {
  const declaration = paramDeclaration(fromLiteral);
  const count = before.split(declaration).length - 1;
  if (count === 0) return { code: SLIDER_GATE_CODES.declarationAbsent, declaration };
  if (count > 1) return { code: SLIDER_GATE_CODES.declarationAmbiguous, declaration, count };
  return before.replace(declaration, () => paramDeclaration(toLiteral));
}

/**
 * A commit's disk effect is exactly the one-span splice `fromLiteral → toLiteral`.
 *
 * @param {{before: string, after: string, fromLiteral: string, toLiteral: string}} args
 */
export function checkCommitRewroteLiteral({ before, after, fromLiteral, toLiteral }) {
  const expected = expectedSourceAfterCommit(before, fromLiteral, toLiteral);
  if (typeof expected !== "string") return [expected];
  if (after === before) return [{ code: SLIDER_GATE_CODES.commitNotWritten, literal: toLiteral }];
  if (after !== expected) return [{ code: SLIDER_GATE_CODES.unexpectedDiff, expected, after }];
  return [];
}

/**
 * A held gesture reached the engine AND left the disk alone. The first half is
 * the vacuity guard: a tool that did nothing would pass the second half too.
 *
 * @param {{baseline: string, afterHold: string, engineMm: number | undefined, heldMm: number}} args
 */
export function checkHeldGestureLeftDiskUntouched({ baseline, afterHold, engineMm, heldMm }) {
  const failures = [];
  if (!isFiniteNumber(engineMm) || Math.abs(engineMm - heldMm) > LENGTH_TOLERANCE_MM) {
    failures.push({ code: SLIDER_GATE_CODES.previewNotLanded, engineMm, heldMm });
  }
  if (afterHold !== baseline) {
    failures.push({ code: SLIDER_GATE_CODES.previewWroteDisk, baseline, afterHold });
  }
  return failures;
}

// ─── The engine ──────────────────────────────────────────────────────────────

/**
 * Millimetre value of `cellId` from an `engine_state` payload
 * (`{values: [{cell_id, si_value, …}]}`), or undefined when it has none.
 * Never throws, for any argument.
 *
 * @param {unknown} engineState
 * @param {string} cellId
 * @returns {number | undefined}
 */
export function cellMm(engineState, cellId) {
  const values = isObject(engineState) ? engineState.values : undefined;
  if (!Array.isArray(values)) return undefined;
  const entry = values.find((v) => isObject(v) && v.cell_id === cellId);
  const si = entry === undefined ? undefined : entry.si_value;
  return isFiniteNumber(si) ? si * 1000 : undefined;
}

// ─── The export ──────────────────────────────────────────────────────────────

const STL_HEADER_BYTES = 80;
const STL_PREAMBLE_BYTES = STL_HEADER_BYTES + 4;
const STL_TRIANGLE_BYTES = 50;
/** Each triangle record opens with its 12-byte normal, then three xyz float32 vertices. */
const STL_NORMAL_BYTES = 12;

/**
 * The triangle count and axis-aligned bounds of a binary STL, or the failure
 * record saying it is not one.
 *
 * @param {Uint8Array} bytes
 * @returns {{triangles: number, min: number[], max: number[]} | {code: string, byteLength: number, triangles?: number}}
 */
export function parseBinaryStl(bytes) {
  const byteLength = bytes.byteLength;
  if (byteLength < STL_PREAMBLE_BYTES) return { code: SLIDER_GATE_CODES.stlMalformed, byteLength };
  const view = new DataView(bytes.buffer, bytes.byteOffset, byteLength);
  const triangles = view.getUint32(STL_HEADER_BYTES, true);
  if (byteLength !== STL_PREAMBLE_BYTES + STL_TRIANGLE_BYTES * triangles) {
    return { code: SLIDER_GATE_CODES.stlMalformed, byteLength, triangles };
  }
  if (triangles === 0) return { code: SLIDER_GATE_CODES.stlEmpty, byteLength, triangles };
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let t = 0; t < triangles; t += 1) {
    const vertices = STL_PREAMBLE_BYTES + STL_TRIANGLE_BYTES * t + STL_NORMAL_BYTES;
    for (let k = 0; k < 9; k += 1) {
      const axis = k % 3;
      const x = view.getFloat32(vertices + 4 * k, true);
      min[axis] = Math.min(min[axis], x);
      max[axis] = Math.max(max[axis], x);
    }
  }
  return { triangles, min, max };
}

/**
 * The exported geometry's X extent is `expectedMm`: the plate is `width` long
 * in X, so this is the param's value as the exporter saw it.
 *
 * @param {{bytes: Uint8Array, expectedMm: number}} args
 */
export function checkExportExtent({ bytes, expectedMm }) {
  const stl = parseBinaryStl(bytes);
  if ("code" in stl) return [stl];
  const observedMm = stl.max[0] - stl.min[0];
  if (Math.abs(observedMm - expectedMm) > STL_EXTENT_TOLERANCE_MM) {
    return [{ code: SLIDER_GATE_CODES.exportExtentMismatch, observedMm, expectedMm }];
  }
  return [];
}

// ─── The controls ────────────────────────────────────────────────────────────

/** The bound joint's slider, scoped to its own mechanism section: exactly one match. */
export function sliderSelector() {
  return `[data-testid="mechanism-section-${MECHANISM_CELL_ID}"] [data-testid="joint-row-${JOINT_INDEX}"] input[type="range"]`;
}

/** The param's PropertyEditor value input: exactly one match. */
export function propInputSelector() {
  return `[data-testid="prop-row-${CELL_ID}"] input[type="text"]`;
}

// ─── Rendering ───────────────────────────────────────────────────────────────

const FAILURE_PROSE = Object.freeze({
  [SLIDER_GATE_CODES.declarationAbsent]: (f) =>
    `the source holds no \`${f.declaration}\`, so no exact splice can be expected`,
  [SLIDER_GATE_CODES.declarationAmbiguous]: (f) =>
    `the source holds \`${f.declaration}\` ${f.count} times, so a splice could not be exact`,
  [SLIDER_GATE_CODES.commitNotWritten]: (f) =>
    `the commit of ${f.literal} left the file on disk byte-identical`,
  [SLIDER_GATE_CODES.unexpectedDiff]: (f) =>
    `the disk changed, but not by exactly the one-span splice.\n--- expected\n${f.expected}\n--- on disk\n${f.after}`,
  [SLIDER_GATE_CODES.previewWroteDisk]: () =>
    "a HELD slider gesture changed the file on disk; only the release may write",
  [SLIDER_GATE_CODES.previewNotLanded]: (f) =>
    `the held value never reached the engine (engine reads ${f.engineMm} mm, held ${f.heldMm} mm), so an untouched disk proves nothing`,
  [SLIDER_GATE_CODES.stlMalformed]: (f) =>
    `the export is not a binary STL (${f.byteLength} bytes${f.triangles === undefined ? "" : `, header claims ${f.triangles} triangles`})`,
  [SLIDER_GATE_CODES.stlEmpty]: () => "the export holds no triangles",
  [SLIDER_GATE_CODES.exportExtentMismatch]: (f) =>
    `the exported X extent is ${f.observedMm} mm, expected ${f.expectedMm} mm`,
});

/**
 * One line per failure record, each prefixed with its code.
 *
 * @param {Array<{code: string}>} failures
 * @returns {string}
 */
export function formatFailures(failures) {
  return failures
    .map((f) => {
      const prose = FAILURE_PROSE[f.code];
      return `[${f.code}] ${prose === undefined ? JSON.stringify(f) : prose(f)}`;
    })
    .join("\n");
}

function isObject(v) {
  return v !== null && typeof v === "object";
}

function isFiniteNumber(v) {
  return typeof v === "number" && Number.isFinite(v);
}
