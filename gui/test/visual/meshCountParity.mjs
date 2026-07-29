/**
 * Cross-layer mesh-count parity: the decision function behind the e2e visual
 * regression gate (task 5367; follow-up from task 5348).
 *
 * ENFORCED INVARIANT — while production demand is SELECTIVE:
 *
 *   viewport_state.meshCount === mesh_stats.meshes.length
 *                            === engine_state.meshes.length
 *
 * Task 5348 routed both debug reads (`engine_state`, `mesh_stats`) through
 * `build_gui_state_full_scene` so they report the whole realized scene rather
 * than only the currently-demanded subset — matching what the viewport actually
 * renders. `docs/debug-mcp-contract.md` asserts that consistency but nothing
 * enforced it; this module is the decision half of the gate that now does.
 *
 * THE VACUITY TRAP (why `fullScope` is an input, not commentary): under
 * `full_scope == true`, `build_gui_state` and `build_gui_state_full_scene`
 * agree BY CONSTRUCTION, so all three counts match trivially and the assertion
 * proves nothing — it would not have caught the 5348 regression. Selectivity is
 * therefore a hard precondition, as is a small body-count floor (otherwise a
 * model that failed to load passes as `0 === 0 === 0`).
 *
 * FAILURES ARE DATA, not sentences: every gate emits a {@link MeshCountFailure}
 * record ({gate, tool, field, observed, expected}) and {@link formatFailures} is
 * the single place one becomes English. That split is what lets the live driver
 * branch on `gate` — an `outage`/`shape` failure means the invariant was never
 * TESTED, a `parity` failure means it was tested and violated — and what keeps
 * this module's suite asserting on observations rather than substring-matching
 * prose, where `toContain("1")` would match nearly any message.
 *
 * PLAIN ESM, NOT TypeScript, and free of `node:` imports — deliberately. Both
 * consumers must be able to load it: the vitest suite
 * (`./meshCountParity.test.ts`, the CI signal) and the live driver
 * (`./smoke_mesh_count_parity_e2e.mjs`), which a runner invokes with bare
 * `node`, not `tsx`.
 *
 * THE TRANSPORT SEAM IS NOT HERE. Decoding a debug-MCP response — the §2a
 * in-band-error discriminator, the §2b `isError` fold, the `tools/call` request
 * shape — lives in `./rpcEnvelope.mjs`, shared with the five sibling smoke
 * drivers (task 5731). This module imports `isInBandError` from there and stops
 * at what its name promises: the parity decision and the extraction that feeds it.
 */

import { isInBandError } from "./rpcEnvelope.mjs";

/**
 * Non-vacuity floor: the minimum `viewport_state.meshCount` a parity run must
 * observe for its three-way equality to mean anything. The committed
 * `gui/test/fixtures/large_assembly.ri` realizes well clear of this.
 */
export const MESH_COUNT_PARITY_MIN_BODIES = 2;

/**
 * Render a rejected value for a failure message without ever throwing.
 *
 * @param {unknown} v
 * @returns {string}
 */
function describeValue(v) {
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "number") return String(v);
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (Array.isArray(v)) return `array(length ${v.length})`;
  return `${typeof v}`;
}

/**
 * A count is usable only if it is a genuine non-negative integer. A missing or
 * mistyped field must surface as a loud, named failure — never coerce, or a
 * dropped field would silently compare as NaN (always unequal, blaming the
 * wrong layer) or as 0 (an all-missing run masquerading as merely degenerate).
 *
 * @param {unknown} v
 * @returns {boolean}
 */
function isUsableCount(v) {
  return typeof v === "number" && Number.isInteger(v) && v >= 0;
}

/** Shape-expectation tokens — see the `MeshCountFailure.expected` table. */
const COUNT = "non-negative-integer";
const ARRAY = "array";
const BOOLEAN = "boolean";
const OBJECT = "object";

/** Token → prose, used only when rendering. */
const SHAPE_PROSE = {
  [COUNT]: "a non-negative integer",
  [ARRAY]: "an array",
  [BOOLEAN]: "a boolean",
  [OBJECT]: "an object",
};

/**
 * @param {string} tool
 * @param {string | undefined} field
 * @param {unknown} observed
 * @param {string} expected  One of the shape-expectation tokens above.
 * @returns {MeshCountFailure}
 */
function shapeFailure(tool, field, observed, expected) {
  return { gate: "shape", tool, ...(field === undefined ? {} : { field }), observed, expected };
}

/** `tool` or `tool.field`, whichever the record identifies. */
function failurePath(f) {
  return f.field === undefined ? String(f.tool) : `${f.tool}.${f.field}`;
}

/**
 * Render failure RECORDS as the human-readable lines a live run prints.
 *
 * The records themselves carry no prose — deliberately. One renderer means the
 * wording can be improved without touching either producer, and it keeps the
 * suite asserting on observations (`f.gate`, `f.observed`) rather than on
 * English, where `toContain("1")` matches almost anything. This is the only
 * place a failure becomes a sentence.
 *
 * Never throws: an unrecognised or malformed record degrades to a dump rather
 * than taking down the diagnostic that was about to explain a failing run.
 *
 * @param {MeshCountFailure[]} failures
 * @returns {string[]} One line per failure, in the order given.
 */
export function formatFailures(failures) {
  if (!Array.isArray(failures)) return [];
  return failures.map((f) => {
    if (f === null || typeof f !== "object") return `malformed failure record: ${describeValue(f)}`;
    switch (f.gate) {
      case "vacuity":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, expected false — this run would be ` +
          `VACUOUS. Under full scope build_gui_state and build_gui_state_full_scene agree by ` +
          `construction, so three-way mesh-count parity is trivially satisfied and proves nothing. ` +
          `Either the frontend never called sync_demand, or a cold eval() reset the scope back to ` +
          `full (engine_eval.rs).`
        );
      case "shape": {
        const want = SHAPE_PROSE[/** @type {string} */ (f.expected)] ?? describeValue(f.expected);
        // "missing or not" when the field simply was not there; a present-but-wrong
        // value reads better as a flat "is not".
        const verb = f.observed === undefined ? "is missing or not" : "is not";
        return `${failurePath(f)} ${verb} ${want}: ${describeValue(f.observed)}`;
      }
      case "degenerate":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, below the non-vacuity floor of ` +
          `${describeValue(f.expected)} bodies — the model failed to load, or realized only part ` +
          `of the fixture, and a three-way match over a truncated scene would pass trivially.`
        );
      case "parity":
        return (
          `${failurePath(f)} (${describeValue(f.observed)}) != viewport_state.meshCount ` +
          `(${describeValue(f.expected)}) under selective demand`
        );
      case "outage":
        return (
          `${failurePath(f)} returned an in-band error: ${describeValue(f.observed)} ` +
          `(docs/debug-mcp-contract.md §2a — a handler failure, not a shape problem)`
        );
      default:
        return `${failurePath(f)}: unrecognised gate ${describeValue(f.gate)} (observed ${describeValue(f.observed)})`;
    }
  });
}

/**
 * @typedef {object} MeshCountParityInputs
 * @property {unknown} viewportMeshCount  `viewport_state.meshCount` — the rendered-scene reference.
 * @property {unknown} meshStatsCount     `mesh_stats.meshes.length`.
 * @property {unknown} engineStateCount   `engine_state.meshes.length`.
 * @property {unknown} fullScope          `demand_dispatch.full_scope` — must be `false`.
 * @property {number} [minBodies]         Non-vacuity floor; anything that is not a
 *                                        non-negative integer falls back to
 *                                        MESH_COUNT_PARITY_MIN_BODIES.
 */

/**
 * @typedef {'vacuity'|'shape'|'degenerate'|'parity'|'outage'} MeshCountFailureGate
 *   Which gate rejected the run. Load-bearing beyond labelling: `outage` and
 *   `shape` mean the invariant was never TESTED (a read failed), while `parity`
 *   means it was tested and violated. A caller that cannot tell those apart
 *   reports a debug-tool outage as a 5348-class cross-layer regression.
 */

/**
 * A single gate violation, as DATA. Prose lives in {@link formatFailures}, not
 * in these records, so a caller can branch on `gate`/`tool` and a test can
 * assert on the observation instead of substring-matching an English sentence.
 *
 * `expected` is gate-specific, and each gate fixes its type:
 *   vacuity    → `false` (the only selective reading)
 *   shape      → an expectation TOKEN: 'non-negative-integer' | 'array' | 'boolean' | 'object'
 *   degenerate → the floor, a number; `observed` must be >= it
 *   parity     → the reference count (`viewport_state.meshCount`)
 *   outage     → absent; the tool failed, so nothing was expected of its value
 *
 * @typedef {object} MeshCountFailure
 * @property {MeshCountFailureGate} gate
 * @property {string} tool      Debug-MCP tool that produced the offending reading.
 * @property {string} [field]   Field path within that tool's payload; absent when
 *                              the whole payload is the problem.
 * @property {unknown} observed What was actually read.
 * @property {unknown} [expected] See the per-gate table above.
 */

/**
 * @typedef {object} MeshCountParityResult
 * @property {boolean} ok                 True only when every gate passed.
 * @property {MeshCountFailure[]} failures Every violation observed, in gate order.
 */

/**
 * Evaluate the parity invariant and both non-vacuity preconditions.
 *
 * ALL gates are evaluated and every violation accumulated — this never
 * early-returns — so one failing live run reports the complete picture instead
 * of forcing a re-run per problem.
 *
 * Never throws, for ANY argument — see the normalisation note below.
 *
 * @param {MeshCountParityInputs} inputs
 * @returns {MeshCountParityResult}
 */
export function checkMeshCountParity(inputs) {
  // Argument normalisation, NOT a stylistic destructuring default: a `= {}`
  // parameter default fires only on `undefined`, so `checkMeshCountParity(null)`
  // would throw a TypeError out of the one function whose entire job is to
  // REPORT unusable readings. `null` is exactly what a caller holds after a
  // failed read, so the throwing path is the one a live outage takes.
  const src = /** @type {any} */ (
    inputs !== null && typeof inputs === "object" ? inputs : {}
  );
  const { viewportMeshCount, meshStatsCount, engineStateCount, fullScope } = src;
  // A non-numeric floor must fall back, never pass through: `1 < null` is false,
  // so a mistyped `minBodies` would SILENTLY DISABLE the degeneracy gate — the
  // failure mode that gate exists to catch, reintroduced by its own parameter.
  const minBodies = isUsableCount(src.minBodies) ? src.minBodies : MESH_COUNT_PARITY_MIN_BODIES;

  /** @type {MeshCountFailure[]} */
  const failures = [];

  // ── Gate 1: vacuity ───────────────────────────────────────────────────────
  // Anything other than an explicit `false` means the run was not taken under
  // selective demand and therefore proves nothing. `observed` keeps the two
  // cases apart for a caller: `true` (scope really was full) vs `undefined` (the
  // scope was never read).
  if (fullScope !== false) {
    failures.push({
      gate: "vacuity",
      tool: "demand_dispatch",
      field: "full_scope",
      observed: fullScope,
      expected: false,
    });
  }

  // ── Gate 2: malformed counts ──────────────────────────────────────────────
  // Checked before parity so an unusable value is reported once, as a shape
  // problem, rather than twice as a confusing inequality against a reference.
  const viewportUsable = isUsableCount(viewportMeshCount);
  const meshStatsUsable = isUsableCount(meshStatsCount);
  const engineStateUsable = isUsableCount(engineStateCount);

  if (!viewportUsable) {
    failures.push(shapeFailure("viewport_state", "meshCount", viewportMeshCount, COUNT));
  }
  if (!meshStatsUsable) {
    failures.push(shapeFailure("mesh_stats", "meshes.length", meshStatsCount, COUNT));
  }
  if (!engineStateUsable) {
    failures.push(shapeFailure("engine_state", "meshes.length", engineStateCount, COUNT));
  }

  // ── Gate 3: degenerate scene ──────────────────────────────────────────────
  if (viewportUsable && viewportMeshCount < minBodies) {
    failures.push({
      gate: "degenerate",
      tool: "viewport_state",
      field: "meshCount",
      observed: viewportMeshCount,
      expected: minBodies,
    });
  }

  // ── Gate 4: parity ────────────────────────────────────────────────────────
  // Each debug read is compared INDEPENDENTLY against the rendered-scene
  // reference so a failing run localises which read drifted.
  if (viewportUsable && meshStatsUsable && meshStatsCount !== viewportMeshCount) {
    failures.push({
      gate: "parity",
      tool: "mesh_stats",
      field: "meshes.length",
      observed: meshStatsCount,
      expected: viewportMeshCount,
    });
  }
  if (viewportUsable && engineStateUsable && engineStateCount !== viewportMeshCount) {
    failures.push({
      gate: "parity",
      tool: "engine_state",
      field: "meshes.length",
      observed: engineStateCount,
      expected: viewportMeshCount,
    });
  }

  return { ok: failures.length === 0, failures };
}

// ─── Payload extraction ──────────────────────────────────────────────────────

/**
 * Validate one tool payload down to a usable object, appending a named failure
 * on any problem.
 *
 * @param {unknown} payload
 * @param {string} toolName  Debug-MCP tool name, used verbatim in failures.
 * @param {MeshCountFailure[]} failures  Accumulator, mutated in place.
 * @returns {Record<string, unknown> | null} The payload, or null if unusable.
 */
function usablePayload(payload, toolName, failures) {
  if (isInBandError(payload)) {
    // `outage`, NOT `shape`: the tool FAILED. A caller must be able to tell that
    // apart from a wrong-shaped answer, because it means the invariant was never
    // tested at all.
    failures.push({
      gate: "outage",
      tool: toolName,
      observed: /** @type {any} */ (payload).error,
    });
    return null;
  }
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    failures.push(shapeFailure(toolName, undefined, payload, OBJECT));
    return null;
  }
  return /** @type {Record<string, unknown>} */ (payload);
}

/**
 * Read `<payload>.meshes.length` defensively.
 *
 * @param {Record<string, unknown> | null} payload
 * @param {string} toolName
 * @param {MeshCountFailure[]} failures
 * @returns {number | undefined}
 */
function meshesLength(payload, toolName, failures) {
  if (payload === null) return undefined;
  const meshes = payload["meshes"];
  if (!Array.isArray(meshes)) {
    failures.push(shapeFailure(toolName, "meshes", meshes, ARRAY));
    return undefined;
  }
  return meshes.length;
}

/**
 * @typedef {object} MeshCountExtraction
 * @property {{viewportMeshCount: number|undefined, meshStatsCount: number|undefined,
 *            engineStateCount: number|undefined, fullScope: boolean|undefined}} inputs
 *           Flat shape accepted by {@link checkMeshCountParity}; a field is
 *           `undefined` exactly when its extraction failed.
 * @property {MeshCountFailure[]} failures Per-tool extraction problems — gate
 *           `outage` when the tool itself failed, `shape` when it answered with
 *           something unreadable.
 */

/**
 * Flatten the four live debug-MCP payloads into the checker's input shape.
 *
 * Shapes read (all verified against base — note the casing seam, the frontend
 * speaks camelCase and Rust speaks snake_case):
 *   `viewport_state`  → `{ meshCount, meshInfo: [{entityPath, …}], … }`  (bridge.ts)
 *   `mesh_stats`      → `{ meshes: [{entity_path, vertex_count, …}] }`   (commands.rs)
 *   `engine_state`    → `{ meshes: [{entity_path, …}], values, … }`      (commands.rs)
 *   `demand_dispatch` → `{ dispatch_by_realization, eval_set, full_scope }` (commands.rs)
 *
 * `viewport_state.meshCount` is `getSceneMeshes().size` — the VISIBILITY-FILTERED
 * scene map, `show`-state meshes only (meshManager.ts; bridge.ts reads it via
 * `vp.getMeshes()`), NOT the whole mesh registry. That filter is why the invariant
 * is conditional on every realized body being visible, as
 * docs/debug-mcp-contract.md spells out. This field is the authoritative count
 * here; `meshInfo` is a per-mesh detail list and is NOT used as the count.
 *
 * Never throws, for ANY argument: every violation becomes a named failure, so a
 * driver can report extraction and parity problems through a single path. That
 * includes the argument itself — `null` (what a caller holds after a failed
 * read) is normalised to "no payloads", which surfaces as four named failures
 * rather than a TypeError, since a `= {}` parameter default fires only on
 * `undefined`.
 *
 * @param {{viewportState?: unknown, meshStats?: unknown, engineState?: unknown,
 *          demandDispatch?: unknown}} payloads
 * @returns {MeshCountExtraction}
 */
export function extractMeshCountInputs(payloads) {
  const { viewportState, meshStats, engineState, demandDispatch } = /** @type {any} */ (
    payloads !== null && typeof payloads === "object" ? payloads : {}
  );

  /** @type {MeshCountFailure[]} */
  const failures = [];

  // viewport_state.meshCount — the rendered-scene reference.
  const viewport = usablePayload(viewportState, "viewport_state", failures);
  /** @type {number | undefined} */
  let viewportMeshCount;
  if (viewport !== null) {
    const raw = viewport["meshCount"];
    if (!isUsableCount(raw)) {
      failures.push(shapeFailure("viewport_state", "meshCount", raw, COUNT));
    } else {
      viewportMeshCount = /** @type {number} */ (raw);
    }
  }

  const meshStatsCount = meshesLength(
    usablePayload(meshStats, "mesh_stats", failures),
    "mesh_stats",
    failures,
  );
  const engineStateCount = meshesLength(
    usablePayload(engineState, "engine_state", failures),
    "engine_state",
    failures,
  );

  // demand_dispatch.full_scope — snake_case, and the ONLY debug read exposing
  // the scope flag (engine_state / mesh_stats deliberately force a full-scope
  // snapshot and so cannot reveal the live scope).
  const dispatch = usablePayload(demandDispatch, "demand_dispatch", failures);
  /** @type {boolean | undefined} */
  let fullScope;
  if (dispatch !== null) {
    const raw = dispatch["full_scope"];
    if (typeof raw !== "boolean") {
      failures.push(shapeFailure("demand_dispatch", "full_scope", raw, BOOLEAN));
    } else {
      fullScope = raw;
    }
  }

  return {
    inputs: { viewportMeshCount, meshStatsCount, engineStateCount, fullScope },
    failures,
  };
}
