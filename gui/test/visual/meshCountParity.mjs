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
 * PLAIN ESM, NOT TypeScript, and free of `node:` imports — deliberately. Both
 * consumers must be able to load it: the vitest suite
 * (`./meshCountParity.test.ts`, the CI signal) and the live driver
 * (`./smoke_mesh_count_parity_e2e.mjs`), which a runner invokes with bare
 * `node`, not `tsx`.
 */

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

/**
 * @typedef {object} MeshCountParityInputs
 * @property {unknown} viewportMeshCount  `viewport_state.meshCount` — the rendered-scene reference.
 * @property {unknown} meshStatsCount     `mesh_stats.meshes.length`.
 * @property {unknown} engineStateCount   `engine_state.meshes.length`.
 * @property {unknown} fullScope          `demand_dispatch.full_scope` — must be `false`.
 * @property {number} [minBodies]         Non-vacuity floor; defaults to MESH_COUNT_PARITY_MIN_BODIES.
 */

/**
 * @typedef {object} MeshCountParityResult
 * @property {boolean} ok        True only when every gate passed.
 * @property {string[]} failures Every violation observed, in gate order.
 */

/**
 * Evaluate the parity invariant and both non-vacuity preconditions.
 *
 * ALL gates are evaluated and every violation accumulated — this never
 * early-returns — so one failing live run reports the complete picture instead
 * of forcing a re-run per problem.
 *
 * @param {MeshCountParityInputs} inputs
 * @returns {MeshCountParityResult}
 */
export function checkMeshCountParity({
  viewportMeshCount,
  meshStatsCount,
  engineStateCount,
  fullScope,
  minBodies = MESH_COUNT_PARITY_MIN_BODIES,
}) {
  /** @type {string[]} */
  const failures = [];

  // ── Gate 1: vacuity ───────────────────────────────────────────────────────
  // Anything other than an explicit `false` means the run was not taken under
  // selective demand and therefore proves nothing.
  if (fullScope !== false) {
    failures.push(
      `demand_dispatch.full_scope is ${describeValue(fullScope)}, expected false — this run would be ` +
        `VACUOUS. Under full scope build_gui_state and build_gui_state_full_scene agree by ` +
        `construction, so three-way mesh-count parity is trivially satisfied and proves nothing. ` +
        `Either the frontend never called sync_demand, or a cold eval() reset the scope back to ` +
        `full (engine_eval.rs).`,
    );
  }

  // ── Gate 2: malformed counts ──────────────────────────────────────────────
  // Checked before parity so an unusable value is reported once, as a shape
  // problem, rather than twice as a confusing inequality against a reference.
  const viewportUsable = isUsableCount(viewportMeshCount);
  const meshStatsUsable = isUsableCount(meshStatsCount);
  const engineStateUsable = isUsableCount(engineStateCount);

  if (!viewportUsable) {
    failures.push(
      `viewport_state.meshCount is not a non-negative integer: ${describeValue(viewportMeshCount)}`,
    );
  }
  if (!meshStatsUsable) {
    failures.push(
      `mesh_stats.meshes.length is not a non-negative integer: ${describeValue(meshStatsCount)}`,
    );
  }
  if (!engineStateUsable) {
    failures.push(
      `engine_state.meshes.length is not a non-negative integer: ${describeValue(engineStateCount)}`,
    );
  }

  // ── Gate 3: degenerate scene ──────────────────────────────────────────────
  if (viewportUsable && viewportMeshCount < minBodies) {
    failures.push(
      `viewport_state.meshCount is ${viewportMeshCount}, below the non-vacuity floor of ${minBodies} ` +
        `bodies — the model almost certainly failed to load, and an all-zero three-way match would ` +
        `pass trivially.`,
    );
  }

  // ── Gate 4: parity ────────────────────────────────────────────────────────
  // Each debug read is compared INDEPENDENTLY against the rendered-scene
  // reference so a failing run localises which read drifted.
  if (viewportUsable && meshStatsUsable && meshStatsCount !== viewportMeshCount) {
    failures.push(
      `mesh_stats.meshes.length (${meshStatsCount}) != viewport_state.meshCount ` +
        `(${viewportMeshCount}) under selective demand`,
    );
  }
  if (viewportUsable && engineStateUsable && engineStateCount !== viewportMeshCount) {
    failures.push(
      `engine_state.meshes.length (${engineStateCount}) != viewport_state.meshCount ` +
        `(${viewportMeshCount}) under selective demand`,
    );
  }

  return { ok: failures.length === 0, failures };
}

// ─── Payload extraction ──────────────────────────────────────────────────────

/**
 * Detect the in-band error shape returned by reify-debug handlers.
 *
 * Debug handlers report failure as `Ok(json!({"error": "<msg>", ...}))` — no MCP
 * `isError` flag is set, so the error rides inside the content block and is
 * indistinguishable from a success value to a naive caller. The inlined `rpc()`
 * helper in every `.mjs` smoke driver only throws on TRANSPORT errors and hands
 * back the parsed text block verbatim, so without this check a tool-level
 * failure would surface as `undefined` counts and get misreported as a shape
 * problem instead of the outage it is.
 *
 * Discriminator: a non-null object whose `error` field is a string. Mirrors
 * `inBandError` in ./rpc.ts and the §2a contract in docs/debug-mcp-contract.md,
 * which is authoritative.
 *
 * NOTE — the OTHER failure dialect: Rust-dispatched tools (`engine_state`,
 * `mesh_stats`, `demand_dispatch`) surface a handler error as an MCP envelope
 * with `isError: true` carrying a plain-text `Error: <msg>` block
 * (debug_server.rs), NOT this JSON shape. The driver's `rpc()` normalises that
 * envelope into `{error: "<text>"}` on the way out precisely so this one
 * detector covers both dialects.
 *
 * Exported so a driver can distinguish "the tool failed" from "the tool
 * answered" BEFORE it starts interpreting fields — see the selectivity
 * precondition in ./smoke_mesh_count_parity_e2e.mjs, where reading a failed
 * `demand_dispatch` as `full_scope !== false` would blame the frontend for a
 * tool outage.
 *
 * @param {unknown} v
 * @returns {boolean}
 */
export function isInBandError(v) {
  return v !== null && typeof v === "object" && typeof (/** @type {any} */ (v).error) === "string";
}

/**
 * @typedef {object} NormalizedRpcEnvelope
 * @property {string} [transportError] Set exactly when the JSON-RPC envelope itself
 *           carried a top-level `error` — the caller should THROW, not interpret.
 * @property {unknown} [payload] The tool's answer, normalised so that BOTH failure
 *           dialects satisfy {@link isInBandError}. `null` means "no text block to
 *           interpret" (e.g. an image content block from `screenshot`).
 */

/**
 * Normalise an MCP `tools/call` response envelope into one interpretable payload.
 *
 * This is the transport seam of the live driver
 * (`./smoke_mesh_count_parity_e2e.mjs`), lifted out of it so the one genuinely
 * subtle branch — the `isError` fold below — is covered by the vitest suite
 * rather than living only in a file that CI can never run.
 *
 * Branch table, in order:
 *   1. top-level `error`      → `{transportError}`; the caller throws. NOT a payload:
 *                               a dead/unreachable server must not read as a tool answer.
 *   2. `result.isError === true` → `{payload: {error: "<text block>"}}` — THE FOLD.
 *                               Rust-dispatched handlers (`engine_state`, `mesh_stats`,
 *                               `demand_dispatch`) report failure this way, with a plain-text
 *                               `Error: <msg>` block (debug_server.rs), while frontend-mediated
 *                               tools use the in-band JSON dialect. Folding the former into the
 *                               latter is what lets ONE `isInBandError` check cover both; without
 *                               it an engine-thread-died failure arrives as the bare string
 *                               "Error: …" and gets misreported downstream as
 *                               `mesh_stats payload is not an object` — a tool OUTAGE dressed up
 *                               as a shape problem.
 *   3. no text block          → `{payload: null}` (image content, empty/absent content).
 *   4. text block, JSON       → `{payload: <parsed>}`. A frontend in-band `{error: "<msg>"}`
 *                               passes through UNCHANGED — it already speaks the target dialect.
 *   5. text block, non-JSON   → `{payload: <raw text>}`.
 *
 * DELIBERATELY NOT `parseRpcResponse` (./rpc.ts), which the typed TS harness uses:
 * that one collapses every failure into `{ok: false, error}`, discarding whether the
 * tool answered at all, and it is TS — unimportable from a bare-`node` `.mjs` driver.
 * This normaliser preserves the in-band shape precisely so `isInBandError` can
 * discriminate outage-from-answer downstream. The overlap is the branch table, and
 * the two are pinned by their own suites (./rpc.test.ts, ./meshCountParity.test.ts).
 *
 * Never throws, whatever the shape of `envelope`.
 *
 * @param {unknown} envelope
 * @returns {NormalizedRpcEnvelope}
 */
export function normalizeRpcEnvelope(envelope) {
  const env = /** @type {any} */ (
    envelope !== null && typeof envelope === "object" ? envelope : {}
  );

  if (env.error) {
    return { transportError: `RPC error: ${JSON.stringify(env.error)}` };
  }

  const content = env.result?.content;
  const textBlock = Array.isArray(content)
    ? content.find((c) => c !== null && typeof c === "object" && c.type === "text")
    : undefined;

  if (env.result?.isError === true) {
    return {
      payload: {
        error:
          typeof textBlock?.text === "string"
            ? textBlock.text
            : "tool reported isError with no text content block",
      },
    };
  }

  if (!textBlock || typeof textBlock.text !== "string") return { payload: null };

  try {
    return { payload: JSON.parse(textBlock.text) };
  } catch {
    return { payload: textBlock.text };
  }
}

/**
 * Validate one tool payload down to a usable object, appending a named failure
 * on any problem.
 *
 * @param {unknown} payload
 * @param {string} toolName  Debug-MCP tool name, used verbatim in failures.
 * @param {string[]} failures  Accumulator, mutated in place.
 * @returns {Record<string, unknown> | null} The payload, or null if unusable.
 */
function usablePayload(payload, toolName, failures) {
  if (isInBandError(payload)) {
    failures.push(
      `${toolName} returned an in-band error: ${JSON.stringify(/** @type {any} */ (payload).error)} ` +
        `(docs/debug-mcp-contract.md §2a — a handler failure, not a shape problem)`,
    );
    return null;
  }
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    failures.push(`${toolName} payload is not an object: ${describeValue(payload)}`);
    return null;
  }
  return /** @type {Record<string, unknown>} */ (payload);
}

/**
 * Read `<payload>.meshes.length` defensively.
 *
 * @param {Record<string, unknown> | null} payload
 * @param {string} toolName
 * @param {string[]} failures
 * @returns {number | undefined}
 */
function meshesLength(payload, toolName, failures) {
  if (payload === null) return undefined;
  const meshes = payload["meshes"];
  if (!Array.isArray(meshes)) {
    failures.push(`${toolName}.meshes is missing or not an array: ${describeValue(meshes)}`);
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
 * @property {string[]} failures Per-tool extraction problems, named by tool.
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
 * Never throws: every violation becomes a named failure, so a driver can report
 * extraction and parity problems through a single path.
 *
 * @param {{viewportState?: unknown, meshStats?: unknown, engineState?: unknown,
 *          demandDispatch?: unknown}} payloads
 * @returns {MeshCountExtraction}
 */
export function extractMeshCountInputs({
  viewportState,
  meshStats,
  engineState,
  demandDispatch,
} = {}) {
  /** @type {string[]} */
  const failures = [];

  // viewport_state.meshCount — the rendered-scene reference.
  const viewport = usablePayload(viewportState, "viewport_state", failures);
  /** @type {number | undefined} */
  let viewportMeshCount;
  if (viewport !== null) {
    const raw = viewport["meshCount"];
    if (!isUsableCount(raw)) {
      failures.push(
        `viewport_state.meshCount is missing or not a non-negative integer: ${describeValue(raw)}`,
      );
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
      failures.push(
        `demand_dispatch.full_scope is missing or not a boolean: ${describeValue(raw)}`,
      );
    } else {
      fullScope = raw;
    }
  }

  return {
    inputs: { viewportMeshCount, meshStatsCount, engineStateCount, fullScope },
    failures,
  };
}
