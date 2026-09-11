/**
 * The decision function behind the printer_v01 Y-rail lengthening integration
 * gate (task 5098; PRD docs/prds/v0_6/ai-native-editing.md §7 leaf ζ).
 *
 * THE SCENARIO. An AI agent lengthens a printer's Y rails through
 * `reify_set_parameter`, in two edits, and the GUI must keep up:
 *
 *   1. `CoreXY.y_rail_len`   800mm -> 1100mm — the rails now overrun the frame,
 *      so the rail-span pin that ties the A-frame to the motion system flips
 *      VIOLATED.
 *   2. `AFrame.rail_span_m`  800mm -> 1100mm — the frame catches up, the pin
 *      returns to SATISFIED, and `AFrame.travel_avail` moves 510mm -> 810mm.
 *
 * WHAT "THE PINS GO GREEN AGAIN" ACTUALLY MEANS — the measured correction this
 * module exists to encode. It is true of the pin PAIR under test and false
 * globally: edit 2's travel_avail move drags the ToolDock's pinned literal
 * `yh_min_today` (145mm) off its `centre_y - travel_avail/2` target, so a
 * SECOND pin legitimately goes red at the last phase. A "zero constraints
 * violated" assertion would therefore red on correct behaviour. This gate names
 * the two pins it is about and demands each one's own status, which turns that
 * cascade into a second, free constraint-status-sync signal.
 *
 * WHY A PIN IS NEVER SELECTED BY `node_id`. `Printer#constraint[45]` is a
 * POSITIONAL key: inserting a constraint anywhere above it in printer.ri
 * silently retargets an index-keyed assertion at a different predicate.
 * `ConstraintData.parameter_ids` is `collect_value_refs(expr)` (engine.rs), so a
 * superset match over the cells a pin is ABOUT names it by meaning instead. Both
 * halves of a `x < y + slack` / `x > y - slack` pair carry the same refs, so the
 * selector matches the pair and {@link foldPinStatus} reduces it — a two-sided
 * pin is green only when both halves hold.
 *
 * FAILURES ARE DATA, not sentences: every gate emits a {@link RailGateFailure}
 * record ({gate, tool, field, observed, expected}) and {@link formatFailures} is
 * the single place one becomes English. That split lets the live driver branch
 * on `gate` — `outage` and `shape` mean the invariant was never TESTED, while
 * `value` and `constraint` mean it was tested and violated — and keeps this
 * module's suite asserting on observations rather than prose.
 *
 * PLAIN ESM, free of builtin-module imports, deliberately: both the vitest suite
 * (`./railLengtheningGate.test.ts`, the CI signal) and the live driver
 * (`./smoke_rail_lengthening_e2e.mjs`, run with bare `node`) must load it. The
 * transport seam — the `tools/call` request shape, the §2a in-band-error
 * discriminator, the §2b `isError` fold — stays in `./rpcEnvelope.mjs`.
 */

import { isInBandError } from "./rpcEnvelope.mjs";

// ─── The subject, its cells, and its pins ────────────────────────────────────

/**
 * The design file under test. Only the BASENAME is stable: the live driver
 * drives a `mkdtemp` copy so the tracked design file is never mutated, and
 * `open_path_into_engine` canonicalizes the path (resolving /tmp's symlink), so
 * the directory that comes back is not the one that went in.
 */
export const SUBJECT_BASENAME = "printer.ri";

/** Where the tracked original lives, relative to the repo root. */
export const PRINTER_RELPATH = "prj/printer_v01/printer.ri";

/** The cell edit 1 rewrites — a `param` of `CoreXY` with a default literal. */
export const Y_RAIL_LEN_CELL = "CoreXY.y_rail_len";
/** The cell edit 2 rewrites. */
export const RAIL_SPAN_CELL = "AFrame.rail_span_m";
/** The DERIVED cell that proves edit 2 propagated: rail_span - brg_len - 2*sock_len. */
export const TRAVEL_AVAIL_CELL = "AFrame.travel_avail";
/** The ToolDock literal that edit 2 knocks out of range — the expected cascade. */
export const YH_MIN_TODAY_CELL = "ToolDock.yh_min_today";
/**
 * The B7 rejection subject: `x_rail_len = y_rail_offset * 2`, a derived `let`
 * with no default literal of its own. A plausible-looking cell id that
 * `reify_set_parameter` must refuse with a structured error, mutating nothing.
 */
export const X_RAIL_LEN_CELL = "CoreXY.x_rail_len";

/**
 * THE SECOND NAMESPACE. The constants above address `engine_state.values`,
 * where a cell id is `<TYPE>.<member>` — `build_values` walks
 * `compiled.templates[].value_cells` and a template is per STRUCTURE, so the
 * entity half is the type name (`CoreXY`, `AFrame`). A constraint's
 * `parameter_ids` is a different thing entirely: `build_constraints` fills it
 * from `collect_value_refs(&c.expr)`, and a `self.<sub>.<member>` access lowers
 * to `ValueCellId::new(format!("{}.{}", scope.entity_name, sub_name), member)`
 * — so the ids are INSTANCE PATHS rooted at the declaring entity
 * (`Printer.a_frame.rail_span_m`), naming the SUB (`motion`), not the type
 * (`CoreXY`).
 *
 * The two namespaces are therefore never interchangeable, and the spellings
 * below are deliberately separate constants rather than a reuse of the `values`
 * ones: selecting a pin with a `values`-namespace id matches nothing, which
 * `foldPinStatus` reports as {@link PIN_ABSENT} — a loud failure at every phase,
 * but one a hand-built fixture reproduces perfectly if it is written from the
 * same wrong assumption. Provenance: crates/reify-compiler/src/expr.rs
 * (`scoped_entity`), gui/src-tauri/src/engine.rs (`build_values`,
 * `build_constraints`), and printer.ri's own `pub structure Printer` block,
 * whose pins read `self.a_frame.rail_span_m < self.motion.y_rail_len + o1_pin_slack`.
 */
export const PIN_RAIL_SPAN_CELL = "Printer.a_frame.rail_span_m";
/** @see PIN_RAIL_SPAN_CELL — the `motion` sub is a `CoreXY`. */
export const PIN_Y_RAIL_LEN_CELL = "Printer.motion.y_rail_len";
/** @see PIN_RAIL_SPAN_CELL */
export const PIN_TRAVEL_AVAIL_CELL = "Printer.a_frame.travel_avail";
/** @see PIN_RAIL_SPAN_CELL */
export const PIN_YH_MIN_TODAY_CELL = "Printer.tool_dock.yh_min_today";

/** Stable NAME of the pin tying the A-frame's rail span to the motion rail length. */
export const RAIL_SPAN_PIN = "rail-span-pin";
/** Stable NAME of the pin tying the ToolDock's Y reach to the A-frame's travel. */
export const TOOL_DOCK_PIN = "tool-dock-pin";

/**
 * How each pin is FOUND: the cells its expression must reference, in the
 * CONSTRAINT namespace documented on {@link PIN_RAIL_SPAN_CELL}. A constraint
 * matches when its `parameter_ids` is a superset of the listed cells, so an
 * expression that also names `Printer.o1_pin_slack` still matches while an
 * unrelated constraint mentioning only one of them does not.
 */
const PIN_SELECTORS = Object.freeze({
  [RAIL_SPAN_PIN]: Object.freeze([PIN_RAIL_SPAN_CELL, PIN_Y_RAIL_LEN_CELL]),
  [TOOL_DOCK_PIN]: Object.freeze([PIN_YH_MIN_TODAY_CELL, PIN_TRAVEL_AVAIL_CELL]),
});

// ─── Non-vacuity floors ──────────────────────────────────────────────────────

/**
 * Minimum realized bodies. A MEASURED lower bound, not a target: one `reify
 * check` of printer.ri named 21 distinct `Entity#realization[i]` in its
 * topology-correspondence log alone, so a scene below that did not finish
 * realizing. Without a floor, a model that failed to load passes every
 * comparison trivially and the gate reports green on nothing at all.
 */
export const RAIL_GATE_MIN_BODIES = 21;

/**
 * Minimum value cells — the three this gate reads. Deliberately a weak floor:
 * the real work is done by the per-cell gates below, which demand each named
 * cell individually. This one only rejects a `values` list that cannot possibly
 * carry them.
 */
export const RAIL_GATE_MIN_VALUES = 3;

/**
 * Minimum constraints — the two PAIRS this gate reads. Same reasoning as
 * {@link RAIL_GATE_MIN_VALUES}: the constraint gates name their pins.
 */
export const RAIL_GATE_MIN_CONSTRAINTS = 4;

/**
 * Minimum entries in `demand_dispatch.dispatch_by_realization`. Orthogonal to
 * the mesh floor on purpose: `engine_state` reports a full-scene snapshot, so a
 * mesh count says a snapshot was BUILT, while this says the demand path
 * actually DISPATCHED.
 */
export const RAIL_GATE_MIN_DISPATCHES = 1;

/**
 * Millimetre tolerance on a value comparison. `si_value` crosses the wire in
 * metres and is scaled by 1000 here, so an exact `===` would red on float noise
 * in a correct run. Far tighter than any real drift: the smallest move this
 * gate asserts is 300mm.
 */
export const RAIL_GATE_LENGTH_TOLERANCE_MM = 1e-6;

// ─── The measured truth table ────────────────────────────────────────────────

/**
 * @typedef {object} RailGatePhase
 * @property {Record<string, number>} cells  Expected millimetre reading per cell.
 * @property {string} railSpanPinStatus      Expected folded status of {@link RAIL_SPAN_PIN}.
 * @property {string} toolDockPinStatus      Expected folded status of {@link TOOL_DOCK_PIN}.
 */

/**
 * The three states the scenario passes through, as MEASURED on printer.ri.
 *
 * `travel_avail = rail_span_m - brg_len_m - 2*sock_len` with brg_len_m 150mm and
 * sock_len 70mm, so 800 - 150 - 140 = 510 and 1100 - 150 - 140 = 810. The second
 * number is what makes the scenario worth gating: the envelope's build_y is
 * 800mm, so 810 is the first travel that covers it.
 *
 * The ToolDock column is the correction described in this module's header — a
 * SECOND pin goes red at the last phase, and that is required, not tolerated.
 *
 * @type {Readonly<Record<string, RailGatePhase>>}
 */
export const RAIL_GATE_PHASES = Object.freeze({
  baseline: Object.freeze({
    cells: Object.freeze({
      [Y_RAIL_LEN_CELL]: 800,
      [RAIL_SPAN_CELL]: 800,
      [TRAVEL_AVAIL_CELL]: 510,
    }),
    railSpanPinStatus: "Satisfied",
    toolDockPinStatus: "Satisfied",
  }),
  "after-y-rail": Object.freeze({
    cells: Object.freeze({
      [Y_RAIL_LEN_CELL]: 1100,
      [RAIL_SPAN_CELL]: 800,
      [TRAVEL_AVAIL_CELL]: 510,
    }),
    railSpanPinStatus: "Violated",
    toolDockPinStatus: "Satisfied",
  }),
  "after-rail-span": Object.freeze({
    cells: Object.freeze({
      [Y_RAIL_LEN_CELL]: 1100,
      [RAIL_SPAN_CELL]: 1100,
      [TRAVEL_AVAIL_CELL]: 810,
    }),
    railSpanPinStatus: "Satisfied",
    toolDockPinStatus: "Violated",
  }),
});

/** The cells every phase carries a reading for, in the order failures report them. */
const TRACKED_CELLS = Object.freeze([Y_RAIL_LEN_CELL, RAIL_SPAN_CELL, TRAVEL_AVAIL_CELL]);

/**
 * The freshness tag a live reading must carry. Anything else means the cell was
 * demand-pruned or failed, and its number is the LAST GOOD value rather than the
 * one this edit produced — which the value gate would happily pass.
 */
const LIVE_FRESHNESS = "final";

/** The status a pin selector reports when nothing matched it. */
export const PIN_ABSENT = "absent";

// ─── Failure records ─────────────────────────────────────────────────────────

/**
 * @typedef {'outage'|'shape'|'vacuity'|'subject'|'stale'|'value'|'constraint'
 *           |'canonical'|'coverage'|'reload'|'rejection'|'read-order'} RailGateFailureGate
 *   Which gate rejected the run. Load-bearing beyond labelling: `outage`,
 *   `shape`, `vacuity` and `read-order` mean the invariant was never TESTED, while
 *   `value` and `constraint` mean it was tested and violated. A caller that cannot
 *   tell those apart reports a debug-tool outage as an AI-write-path regression.
 *
 * @typedef {object} RailGateFailure
 * @property {RailGateFailureGate} gate
 * @property {string} tool       Debug-MCP tool that produced the offending reading.
 * @property {string} [field]    Field path within that payload; absent when the
 *                               whole payload is the problem.
 * @property {unknown} observed  What was actually read.
 * @property {unknown} [expected] Gate-specific; absent for `outage`, where the
 *                               tool failed and nothing was expected of its value.
 */

/** Shape-expectation tokens — see {@link SHAPE_PROSE}. */
const ARRAY = "array";
const OBJECT = "object";
const NUMBER = "finite-number";
const NON_EMPTY_STRING = "non-empty-string";
const KNOWN_PHASE = "one-of-RAIL_GATE_PHASES";
const IN_BAND_ERROR = "in-band-error-envelope";
const NON_EMPTY_OBJECT = "object-with-at-least-one-entry";

/** Token → prose, used only when rendering. */
const SHAPE_PROSE = Object.freeze({
  [ARRAY]: "an array",
  [OBJECT]: "an object",
  [NUMBER]: "a finite number",
  [NON_EMPTY_STRING]: "a non-empty string",
  [KNOWN_PHASE]: `one of ${Object.keys(RAIL_GATE_PHASES).join(", ")}`,
  [IN_BAND_ERROR]: "an in-band {error} envelope (docs/debug-mcp-contract.md §2a)",
  [NON_EMPTY_OBJECT]: "an object carrying at least one entry to compare",
});

/**
 * Render a rejected value for a failure message without ever throwing.
 *
 * @param {unknown} v
 * @returns {string}
 */
function describeValue(v) {
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "number") return String(v);
  // Booleans are not a formality here: `stale`, `reload_error` and
  // `reify_save_file.success` are the boolean-valued readings, so without this
  // branch the only diagnostic the live gate prints for them reads
  // "stale is boolean, expected boolean" — both numbers erased.
  if (typeof v === "boolean") return String(v);
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (Array.isArray(v)) return `array(length ${v.length})`;
  return `${typeof v}`;
}

/**
 * @param {string} tool
 * @param {string | undefined} field
 * @param {unknown} observed
 * @param {string} expected  One of the shape-expectation tokens above.
 * @returns {RailGateFailure}
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
 * The records carry no prose — deliberately. One renderer means the wording can
 * be improved without touching either producer, and it keeps the suite asserting
 * on observations rather than on English. Never throws: a malformed record
 * degrades to a dump rather than taking down the diagnostic that was about to
 * explain a failing run.
 *
 * @param {RailGateFailure[]} failures
 * @returns {string[]} One line per failure, in the order given.
 */
export function formatFailures(failures) {
  if (!Array.isArray(failures)) return [];
  return failures.map((f) => {
    if (f === null || typeof f !== "object") return `malformed failure record: ${describeValue(f)}`;
    switch (f.gate) {
      case "outage":
        return (
          `${failurePath(f)} returned an in-band error: ${describeValue(f.observed)} ` +
          `(docs/debug-mcp-contract.md §2a — the tool FAILED, so this run never TESTED the ` +
          `invariant; it is not evidence either way)`
        );
      case "shape": {
        // OWN keys only, and strings only: a bare `SHAPE_PROSE[f.expected]`
        // resolves an INHERITED one, so `expected: 'constructor'` renders the
        // Object constructor's source where a token's prose belongs — the
        // opposite of the "a malformed record degrades to a dump" contract.
        const want =
          typeof f.expected === "string" &&
          Object.prototype.hasOwnProperty.call(SHAPE_PROSE, f.expected)
            ? SHAPE_PROSE[f.expected]
            : describeValue(f.expected);
        const verb = f.observed === undefined ? "is missing or not" : "is not";
        return `${failurePath(f)} ${verb} ${want}: ${describeValue(f.observed)}`;
      }
      case "vacuity":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, below the non-vacuity floor of ` +
          `${describeValue(f.expected)} — printer.ri did not finish realizing, and every ` +
          `comparison below it would pass trivially on an empty scene`
        );
      case "subject":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, which does not end in ` +
          `${describeValue(f.expected)} — this run graded some other file`
        );
      case "stale":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, expected ` +
          `${describeValue(f.expected)} — the cell was demand-pruned or failed, so its number is ` +
          `the LAST GOOD value rather than the one this edit produced, and the value gate would ` +
          `pass on it`
        );
      case "value":
        return (
          `${failurePath(f)} reads ${describeValue(f.observed)}mm, expected ` +
          `${describeValue(f.expected)}mm — the AI edit did not reach this cell`
        );
      case "constraint":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, expected ` +
          `${describeValue(f.expected)} — the constraint-status sync did not carry this pin's ` +
          `flip across the write-tool payload (INV-GUI-2)`
        );
      case "canonical":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, expected ` +
          `${describeValue(f.expected)} — the AI edit did not land in the SOURCE, so the value ` +
          `the engine reports is ephemeral and the next reload would lose it (INV-GUI-3)`
        );
      case "coverage":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, expected ` +
          `${describeValue(f.expected)} — a GuiState field beyond meshes/values did not survive ` +
          `the AI edit, or the reload that produced it failed and every field is LAST GOOD ` +
          `(INV-GUI-1)`
        );
      case "reload":
        return (
          `${failurePath(f)} moved to ${describeValue(f.observed)} from ${describeValue(f.expected)} ` +
          `ACROSS the watcher debounce — the write was applied twice, or the reload is looping ` +
          `(PRD §7 B5)`
        );
      case "rejection":
        return (
          `${failurePath(f)} is ${describeValue(f.observed)}, expected ${describeValue(f.expected)} ` +
          `— a refused write must fail structurally and leave disk, source_map and engine ` +
          `byte-identical (§6.1 atomicity)`
        );
      case "read-order":
        return (
          `${failurePath(f)} was ${describeValue(f.observed)}, expected ${describeValue(f.expected)} ` +
          `— an object literal in argument position is evaluated, its awaits included, BEFORE the ` +
          `callee runs, so those reads would precede the phase's own and the reify_open_file among ` +
          `them would reload the file from disk (debug_server.rs:1525), making PRD §7 B1's ` +
          `"without a file reload" a tautology`
        );
      default:
        return `${failurePath(f)}: unrecognised gate ${describeValue(f.gate)} (observed ${describeValue(f.observed)})`;
    }
  });
}

// ─── Pin selection ───────────────────────────────────────────────────────────

/**
 * Every constraint whose `parameter_ids` is a superset of `cells`.
 *
 * Returns a LIST because a `±slack` pin is written as two constraints sharing
 * the same refs; see {@link foldPinStatus}. Never throws — a malformed
 * `constraints` list or entry yields no matches rather than an exception.
 *
 * @param {unknown} constraints  `engine_state.constraints`.
 * @param {readonly string[]} cells  The cells the pin must reference.
 * @returns {any[]}
 */
export function selectPinConstraints(constraints, cells) {
  if (!Array.isArray(constraints) || !Array.isArray(cells)) return [];
  return constraints.filter((c) => {
    if (c === null || typeof c !== "object") return false;
    const ids = /** @type {any} */ (c).parameter_ids;
    if (!Array.isArray(ids)) return false;
    return cells.every((cell) => ids.includes(cell));
  });
}

/**
 * Reduce the halves of one pin to a single status.
 *
 * A two-sided pin holds only when BOTH halves hold, so any `Violated` half makes
 * the pin violated; failing that, any `Indeterminate` half makes it
 * indeterminate. No match at all is {@link PIN_ABSENT} rather than a silent
 * pass — a pin that vanished from printer.ri must fail the gate loudly, which is
 * exactly what an index-keyed selector would have hidden.
 *
 * @param {any[]} matched  Output of {@link selectPinConstraints}.
 * @returns {string}
 */
export function foldPinStatus(matched) {
  if (!Array.isArray(matched) || matched.length === 0) return PIN_ABSENT;
  const statuses = matched.map((c) =>
    c !== null && typeof c === "object" ? /** @type {any} */ (c).status : undefined,
  );
  if (statuses.some((s) => s === "Violated")) return "Violated";
  if (statuses.some((s) => s === "Indeterminate")) return "Indeterminate";
  if (statuses.every((s) => s === "Satisfied")) return "Satisfied";
  // A status outside the {Satisfied, Violated, Indeterminate} vocabulary
  // (engine.rs build_constraints) is not a pin reading at all.
  return PIN_ABSENT;
}

// ─── The write path's static precondition ────────────────────────────────────

/**
 * A `.ri` entity header — `pub structure Foo {`, or `structure def Foo : Bar {`.
 * The `def` form is a definition-typed structure and still opens an entity body.
 */
const ENTITY_OPEN = /^\s*(?:pub\s+)?structure\s+(?:def\s+)?([A-Za-z_]\w*)\b/;

/**
 * A member declaration: the keyword, the name, an optional `: Type` annotation,
 * and the default literal when one follows.
 *
 * `[^=]*` on the annotation run means the FIRST `=` on the line always opens the
 * default group — including one buried inside a type argument. Measured:
 * `param p : Vec<N = 3>` reads as a param whose default literal is `3>`. That is
 * the false-PASS direction for the `hasDefaultLiteral` half of the precondition,
 * and it is accepted rather than narrowed because telling the two apart means
 * parsing type arguments, and no `.ri` type syntax carries an `=` today (the
 * generic annotations in the tree — `Option<Pressure>`, `Result<Length, String>`
 * — do not). The cells this module is asked about are annotated `: Length`.
 *
 * The trailing group captures the literal TEXT — `800mm`, not just "there was
 * one" — because B2 is a unit-PRESERVING assertion: `1.1m` and `1100mm` are the
 * same length and different literals, and only the second is what
 * `reify_set_parameter` was asked to write.
 */
const MEMBER_DECL = /^\s*(param|let)\s+([A-Za-z_]\w*)\s*(?::[^=]*)?(?:=\s*(.*?))?\s*$/;

/**
 * Index every top-level `param`/`let` in a `.ri` source, keyed `Entity.member`.
 *
 * A SCAN, NOT A PARSE — and the limits are the point rather than an oversight.
 * Comments are blanked before brace counting (a `{` in prose would otherwise
 * desynchronise the depth), but a brace inside a string literal is not seen; a
 * `.ri` source carrying one would mis-scope the members after it. Only depth-1
 * declarations count, so a `let` inside a `realize` block is not a member of the
 * entity — and an entity whose opening `{` sits on the line AFTER its header
 * loses every member, because the end-of-line `depth <= 0` reset clears the
 * entity before its body ever opens. The first declaration of a name wins.
 *
 * The failure mode is benign in both directions: a member the scan misses reads
 * as `absent`, and a mis-scoped one reads under the wrong entity — either way
 * the precondition check below FAILS rather than silently passing, which is the
 * direction a gate must err in.
 *
 * @param {unknown} source
 * @returns {Map<string, {declared: string, hasDefaultLiteral: boolean, defaultLiteral: string|undefined}>}
 */
function scanDeclarations(source) {
  /** @type {Map<string, {declared: string, hasDefaultLiteral: boolean, defaultLiteral: string|undefined}>} */
  const declarations = new Map();
  if (typeof source !== "string") return declarations;

  const lines = source
    .replace(/\/\*[\s\S]*?\*\//g, (span) => span.replace(/[^\n]/g, " "))
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, ""));

  let entity = null;
  let depth = 0;
  for (const line of lines) {
    if (entity === null) {
      const open = ENTITY_OPEN.exec(line);
      if (open !== null) {
        entity = open[1];
        depth = 0;
      }
    } else if (depth === 1) {
      const decl = MEMBER_DECL.exec(line);
      if (decl !== null) {
        const key = `${entity}.${decl[2]}`;
        if (!declarations.has(key)) {
          declarations.set(key, {
            declared: /** @type {string} */ (decl[1]),
            hasDefaultLiteral: decl[3] !== undefined,
            defaultLiteral: decl[3],
          });
        }
      }
    }
    // Brace accounting comes AFTER the reads above, so the entity's own opening
    // `{` leaves the NEXT line — the first member — at depth 1.
    for (const ch of line) {
      if (ch === "{") depth += 1;
      else if (ch === "}") depth -= 1;
    }
    if (entity !== null && depth <= 0) entity = null;
  }
  return declarations;
}

/**
 * @typedef {object} EditableParamRecord
 * @property {unknown} cell   The requested cell id, echoed verbatim.
 * @property {'param'|'let'|'absent'} declared  How the source declares it.
 * @property {boolean} hasDefaultLiteral  Whether the declaration carries an `=`.
 */

/**
 * How `source` declares each requested cell — the precondition
 * `reify_set_parameter` imposes, checked statically.
 *
 * That tool rewrites a parameter's DEFAULT LITERAL through alpha's
 * `resolve_param_default_span`, which returns None for a cell that is not a
 * `param` with a default, and the write is then refused. A cell is writable
 * exactly when `declared === 'param' && hasDefaultLiteral`; every other
 * combination names a different reason it is not, which is what makes this
 * usable for BOTH halves of the gate — the two cells that must be editable, and
 * the derived `let` that must stay a rejection subject.
 *
 * Returns one record per request, in the order asked, so a caller can zip the
 * result against its own list. Never throws for any argument.
 *
 * @param {unknown} source
 * @param {unknown} cellNames
 * @returns {EditableParamRecord[]}
 */
export function findEditableParams(source, cellNames) {
  if (!Array.isArray(cellNames)) return [];
  const declarations = scanDeclarations(source);
  return cellNames.map((cell) => {
    const found = typeof cell === "string" ? declarations.get(cell) : undefined;
    if (found === undefined) return { cell, declared: "absent", hasDefaultLiteral: false };
    return { cell, declared: found.declared, hasDefaultLiteral: found.hasDefaultLiteral };
  });
}

// ─── The remaining PRD §7 rows ───────────────────────────────────────────────
//
// B4 is not here, and its absence is deliberate rather than an omission. Its
// postcondition is about a `StateDelta`, and PRD §6.2 caveat (i) — restated on
// `write_on_engine_and_refresh_baseline` — says the debug path DISCARDS the
// delta and pushes the full `GuiState` instead. So no debug tool can hand a
// delta to a predicate here, and B4 is asserted where `compute_delta` and
// `last_state` both are, against the real write seam: `debug_server::tests::
// write_tools::write_helper_refreshes_the_delta_baseline`.
//
// Each predicate below returns a plain failure LIST rather than a verdict, so
// {@link checkRailLengtheningGate} can fold them all into one `{ok, failures}`
// and the driver keeps a single decision seam and a single rendering site.

/**
 * PRD §7 B2 — the AI edit is canonical ON DISK (INV-GUI-3).
 *
 * `reify_set_parameter` rewrites a param's DEFAULT LITERAL, so the assertion is
 * on the literal TEXT and is unit-preserving: `1.1m` is the same length as
 * `1100mm` and is not what the tool was asked to write. A cell that is not a
 * `param` with a default reports how it IS declared (`let`, `absent`), because
 * that names the reason the write could not have landed rather than merely
 * reporting a missing string.
 *
 * The no-op half is asserted as source BYTE-IDENTITY either side of the save,
 * not by reading a flag: `reify_save_file`'s envelope is a bare
 * `{success: true}` (debug_server.rs `reify_save_file_envelope`) and carries no
 * "changed" field to read instead.
 *
 * @param {{source?: unknown, expected?: unknown, saveFile?: unknown,
 *          sourceAfterSave?: unknown}} observed
 * @returns {RailGateFailure[]}
 */
export function checkSourceCanonical(observed) {
  const src = asObject(observed) ?? {};
  /** @type {RailGateFailure[]} */
  const failures = [];

  if (typeof src.source !== "string" || src.source.length === 0) {
    failures.push(shapeFailure("reify_open_file", "source", src.source, NON_EMPTY_STRING));
  }

  // A B2 reading with nothing to compare is a VACUITY, not a pass: without this
  // a missing, null or array-valued `expected` drops the entire literal-
  // canonicity assertion — the whole point of the row — and the predicate
  // returns clean having checked only the save.
  const expected = asObject(src.expected);
  if (expected === null || Object.keys(expected).length === 0) {
    failures.push(
      shapeFailure("railLengtheningGate", "sourceCanonical.expected", src.expected, NON_EMPTY_OBJECT),
    );
  }

  const declarations = scanDeclarations(src.source);
  for (const [cell, want] of Object.entries(expected ?? {})) {
    const found = declarations.get(cell);
    // One reading, three reasons it can differ: wrong literal, wrong keyword,
    // or not there at all. Reporting them in the same slot keeps the record
    // shape uniform while still naming which one happened.
    const reading =
      found === undefined
        ? PIN_ABSENT
        : found.declared !== "param" || found.defaultLiteral === undefined
          ? found.declared
          : found.defaultLiteral;
    if (reading !== want) {
      failures.push({
        gate: "canonical",
        tool: "reify_open_file",
        field: `source[${cell}]`,
        observed: reading,
        expected: want,
      });
    }
  }

  const save = usablePayload(src.saveFile, "reify_save_file", failures);
  if (save !== null) {
    if (save.success !== true) {
      failures.push({
        gate: "canonical",
        tool: "reify_save_file",
        field: "success",
        observed: save.success,
        expected: true,
      });
    }
    if (src.sourceAfterSave !== src.source) {
      failures.push({
        gate: "canonical",
        tool: "reify_save_file",
        field: "source-after-save",
        observed: src.sourceAfterSave,
        expected: src.source,
      });
    }
  }
  return failures;
}

/** `engine_state` list fields that are neither `meshes` nor `values`. */
const COVERAGE_LIST_FIELDS = Object.freeze([
  "constraints",
  "files",
  "compile_diagnostics",
  "tessellation_diagnostics",
]);

/**
 * Of those, the ones printer.ri must actually populate. The two diagnostics
 * lists are legitimately EMPTY on a clean design, so requiring content from
 * them would red the gate on a design with nothing wrong with it.
 */
const COVERAGE_NON_EMPTY_FIELDS = Object.freeze(["constraints", "files"]);

/** What "populated" means for those — presence of any entry at all. */
const COVERAGE_MIN_ENTRIES = 1;

/**
 * PRD §7 B3 — fields beyond `meshes`/`values` stay live across the AI edit
 * (INV-GUI-1).
 *
 * `stale` / `reload_error` are checked alongside the lists, and they are the
 * load-bearing half: a populated field proves nothing if the reload that would
 * have refreshed it FAILED, because then every field is the last good one and
 * a presence check passes on stale data — the same trap `LIVE_FRESHNESS` closes
 * for individual cells.
 *
 * @param {unknown} engineState  The `engine_state` payload.
 * @returns {RailGateFailure[]}
 */
export function checkFieldCoverage(engineState) {
  /** @type {RailGateFailure[]} */
  const failures = [];
  const engine = usablePayload(engineState, "engine_state", failures);
  if (engine === null) return failures;

  for (const field of COVERAGE_LIST_FIELDS) {
    const value = engine[field];
    if (!Array.isArray(value)) {
      failures.push(shapeFailure("engine_state", field, value, ARRAY));
      continue;
    }
    if (COVERAGE_NON_EMPTY_FIELDS.includes(field) && value.length === 0) {
      failures.push({
        gate: "coverage",
        tool: "engine_state",
        field,
        observed: value.length,
        expected: COVERAGE_MIN_ENTRIES,
      });
    }
  }

  for (const [field, observedValue, want] of /** @type {const} */ ([
    ["stale", engine.stale, false],
    ["reload_error", engine.reload_error, null],
  ])) {
    if (observedValue !== want) {
      failures.push({
        gate: "coverage",
        tool: "engine_state",
        field,
        observed: observedValue,
        expected: want,
      });
    }
  }
  return failures;
}

/**
 * The projection B5 compares — every tracked cell's `{mm, freshness}` and both
 * pin statuses, flattened to `field -> value` — or `null`, having faulted by
 * name whatever `reading` did not carry.
 *
 * BOTH SIDES GO THROUGH HERE, and that is what makes the comparison below an
 * equality over PRESENT data. Reading each key straight off two raw objects
 * compares `undefined` with `undefined` wherever a reading is missing, so a pair
 * of empty observations agrees on every key and B5 grades clean having observed
 * nothing at all — the same vacuity the gate's own floors exist to close.
 *
 * @param {unknown} reading
 * @param {string} side  `before` or `after`, used verbatim in failure fields.
 * @param {RailGateFailure[]} failures  Accumulator, mutated in place.
 * @returns {Record<string, unknown> | null}
 */
function railProjection(reading, side, failures) {
  const mark = failures.length;
  const src = asObject(reading);
  if (src === null) {
    failures.push(shapeFailure("engine_state", side, reading, OBJECT));
    return null;
  }
  const cells = asObject(src.cells) ?? {};

  /** @type {Record<string, unknown>} */
  const projection = {};
  for (const cell of TRACKED_CELLS) {
    const entry = asObject(cells[cell]);
    if (entry === null) {
      failures.push(shapeFailure("engine_state", `${side}.values[${cell}]`, cells[cell], OBJECT));
      continue;
    }
    projection[`values[${cell}].mm`] = entry.mm;
    projection[`values[${cell}].freshness`] = entry.freshness;
  }
  for (const [pin, key] of /** @type {const} */ ([
    [RAIL_SPAN_PIN, "railSpanPinStatus"],
    [TOOL_DOCK_PIN, "toolDockPinStatus"],
  ])) {
    const status = src[key];
    if (typeof status !== "string" || status.length === 0) {
      failures.push(
        shapeFailure("engine_state", `${side}.constraints[${pin}].status`, status, NON_EMPTY_STRING),
      );
      continue;
    }
    projection[`constraints[${pin}].status`] = status;
  }
  // A projection is returned only when nothing was faulted, so the caller never
  // compares a partial reading against a complete one.
  return failures.length === mark ? projection : null;
}

/**
 * PRD §7 B5 — the FS watcher's post-debounce re-read adds no churn.
 *
 * The AI write already wrote disk, so the watcher re-read must be a no-op: any
 * cell, freshness or pin that MOVES across the debounce window is a double
 * apply or a reload loop. Compares the two observations against each other
 * rather than against the phase table on purpose — B5 is about the difference
 * being empty, whatever the values are, so it still holds at a phase whose
 * expected numbers are themselves wrong.
 *
 * @param {{before?: unknown, after?: unknown}} observed
 * @returns {RailGateFailure[]}
 */
export function checkIdempotentReload(observed) {
  const src = asObject(observed) ?? {};
  /** @type {RailGateFailure[]} */
  const failures = [];
  const before = railProjection(src.before, "before", failures);
  const after = railProjection(src.after, "after", failures);
  if (before === null || after === null) return failures;

  for (const field of Object.keys(before)) {
    // `Object.is` so a NaN reading on both sides reads as unchanged rather than
    // as churn it is not.
    if (!Object.is(after[field], before[field])) {
      failures.push({
        gate: "reload",
        tool: "engine_state",
        field,
        observed: after[field],
        expected: before[field],
      });
    }
  }
  return failures;
}

/**
 * PRD §7 B7 — a refused write fails structurally and mutates nothing.
 *
 * Both halves matter and they fail independently, so both are always evaluated:
 * a write that SUCCEEDED where a rejection was required is as much a B7
 * violation as one that left the source half-rewritten, and a run that did both
 * reports two records rather than the first one found.
 *
 * "Structured" is the §2a in-band `{error}` envelope carrying a non-empty
 * message — an empty one is a refusal the caller cannot act on or diagnose.
 *
 * The atomicity half needs BOTH sources to be real readings before comparing
 * them: `undefined !== undefined` is false, so a `reify_open_file` answer that
 * is well-shaped but carries no `source` string graded a full B7 pass having
 * observed the file on neither side.
 *
 * @param {{tool?: unknown, error?: unknown, sourceBefore?: unknown,
 *          sourceAfter?: unknown}} observed
 * @returns {RailGateFailure[]}
 */
export function checkRejectionAtomicity(observed) {
  const src = asObject(observed) ?? {};
  /** @type {RailGateFailure[]} */
  const failures = [];
  const tool = typeof src.tool === "string" && src.tool.length > 0 ? src.tool : "reify_set_parameter";

  if (!isInBandError(src.error)) {
    failures.push({
      gate: "rejection",
      tool,
      field: "error",
      observed: src.error,
      expected: IN_BAND_ERROR,
    });
  } else if (/** @type {any} */ (src.error).error.trim().length === 0) {
    failures.push({
      gate: "rejection",
      tool,
      field: "error",
      observed: /** @type {any} */ (src.error).error,
      expected: NON_EMPTY_STRING,
    });
  }

  let bothRead = true;
  for (const [field, value] of /** @type {const} */ ([
    ["sourceBefore", src.sourceBefore],
    ["sourceAfter", src.sourceAfter],
  ])) {
    if (typeof value !== "string" || value.length === 0) {
      failures.push(shapeFailure(tool, field, value, NON_EMPTY_STRING));
      bothRead = false;
    }
  }

  if (bothRead && src.sourceAfter !== src.sourceBefore) {
    failures.push({
      gate: "rejection",
      tool,
      field: "source",
      observed: src.sourceAfter,
      expected: src.sourceBefore,
    });
  }
  return failures;
}

// ─── The read-order seam ─────────────────────────────────────────────────────

/** What `gatherExtras` must be. Stated once; the record's `expected` reads it. */
const EXTRAS_THUNK = "a thunk — see observeThenExtras";

/** The {@link RailGateInputs} field a read-order problem is parked in. */
const READ_ORDER = "readOrder";

/** `err`'s message, or a description of whatever non-Error was thrown. */
function throwReason(err) {
  return err instanceof Error && typeof err.message === "string" ? err.message : describeValue(err);
}

/** A marker the verdict renders as a `read-order` record. */
function readOrderMarker(field, observed) {
  return { [READ_ORDER]: { field, observed } };
}

/**
 * Observe one phase's state, THEN gather whatever extra readings it needs.
 *
 * THE MECHANISM THIS EXISTS TO CLOSE. An object literal passed as an ARGUMENT is
 * fully evaluated — its `await`s included — before the callee runs. So
 *
 *     gradePhase(phase, subject, {sourceCanonical: await readSourceCanonical(…)})
 *
 * issues every read inside the literal FIRST, and `readSourceCanonical`'s chain
 * is `reify_open_file` -> `reify_save_file` -> `reify_open_file`. The first of
 * those reaches `open_path_into_engine` (debug_server.rs:1525), which re-reads
 * the file from disk and refreshes the baseline — a full reload. Every reading
 * the phase then takes describes a freshly reloaded engine, so PRD §7 B1 ("the
 * viewport and property panel follow WITHOUT a file reload") passes no matter
 * what the AI write did. The failure is silent and in the direction that PASSES.
 *
 * `observePhase`'s READ ORDER IS LOAD-BEARING paragraph in
 * `./smoke_rail_lengthening_e2e.mjs` states that invariant; this function is
 * where it is ENFORCED, because a driver needs a live reify-gui to run at all
 * and this module is the only half of the gate CI can execute. Heuristic 10:
 * enforced where it can be, stated where it must be.
 *
 * NEVER REJECTS, for any pair of arguments — a rejecting thunk, a non-thunk, a
 * symbol — because the caller holding the result is a live driver whose job is
 * to REPORT. Each problem becomes a {@link READ_ORDER} marker that
 * {@link checkRailLengtheningGate} turns into a `read-order` record, so the
 * disarmed spelling reds the run instead of quietly reordering it.
 *
 * @param {() => unknown} observe       Takes the phase's own readings.
 * @param {(() => unknown) | undefined} [gatherExtras]  Extra readings, or
 *   nothing — a phase with no extra PRD row to grade legitimately passes none.
 * @returns {Promise<Record<string, unknown>>} The observation with the resolved
 *   extras merged OVER it, so an extra wins a key collision.
 */
export async function observeThenExtras(observe, gatherExtras) {
  if (typeof observe !== "function") {
    return readOrderMarker("observe", describeValue(observe));
  }
  let inputs;
  try {
    inputs = await observe();
  } catch (err) {
    return readOrderMarker("observe", `a thunk that threw: ${throwReason(err)}`);
  }

  if (gatherExtras === undefined) return { ...asObject(inputs) };
  if (typeof gatherExtras !== "function") {
    // NOT merged, deliberately. Merging it would produce exactly the verdict the
    // thunk form produces, leaving the reordering to be noticed by nobody.
    return { ...asObject(inputs), ...readOrderMarker("extras", typeof gatherExtras) };
  }
  try {
    // The await-then-await IS the contract. A `Promise.all` here, or hoisting
    // either call above this line, reinstates the interleaving described above.
    const extras = await gatherExtras();
    return { ...asObject(inputs), ...asObject(extras) };
  } catch (err) {
    return { ...asObject(inputs), ...readOrderMarker("extras", `a thunk that threw: ${throwReason(err)}`) };
  }
}

/**
 * Render a {@link READ_ORDER} marker as the one record it stands for.
 *
 * A marker is only ever produced by {@link observeThenExtras}, so a reading that
 * is not marker-shaped is itself the anomaly and is reported rather than
 * skipped; `null`/`undefined` mean "no problem" and grade clean.
 *
 * @param {unknown} observed
 * @returns {RailGateFailure[]}
 */
export function checkReadOrder(observed) {
  if (observed === null || observed === undefined) return [];
  const src = asObject(observed) ?? {};
  return [
    {
      gate: "read-order",
      tool: "railLengtheningGate",
      field: typeof src.field === "string" ? src.field : "extras",
      observed: "observed" in src ? src.observed : describeValue(observed),
      expected: EXTRAS_THUNK,
    },
  ];
}

/**
 * The PRD rows that are checked only when the caller supplies their reading —
 * name → the {@link RailGateInputs} field carrying it, and the predicate.
 *
 * `list` marks a row a run can exercise more than once: B7 is driven twice (a
 * derived `let`, then a dimension-mismatched value), and both rejections must be
 * graded rather than only the last one supplied.
 *
 * {@link READ_ORDER} is the one row that is NOT a PRD row: it grades the harness
 * rather than the subject. It rides the same "a supplied reading is what asks to
 * be graded" mechanism because that is precisely its shape — a clean run parks
 * no marker and the row never fires.
 */
const EXTRA_GATES = Object.freeze({
  sourceCanonical: { check: checkSourceCanonical, list: false },
  fieldCoverage: { check: checkFieldCoverage, list: false },
  idempotentReload: { check: checkIdempotentReload, list: false },
  rejectionAtomicity: { check: checkRejectionAtomicity, list: true },
  [READ_ORDER]: { check: checkReadOrder, list: false },
});

/**
 * The rows a run may PROMISE to exercise — every {@link EXTRA_GATES} row except
 * {@link READ_ORDER}.
 *
 * `readOrder` is excluded because promising it is never satisfiable: a healthy
 * run parks no marker, so `requires: ['readOrder']` would fail every correct
 * run. It grades the harness, not the subject, and rides EXTRA_GATES only for
 * the "a supplied reading is what asks to be graded" half of the mechanism.
 */
const REQUIRABLE_ROWS = Object.freeze(Object.keys(EXTRA_GATES).filter((n) => n !== READ_ORDER));

/** Shape token for an unrecognised entry in `requires`. */
const KNOWN_EXTRA = `one-of-${REQUIRABLE_ROWS.join("|")}`;

// ─── The verdict ─────────────────────────────────────────────────────────────

/**
 * @typedef {object} RailGateCellReading
 * @property {unknown} mm         Millimetre value, scaled from `si_value`.
 * @property {unknown} freshness  `ValueData.freshness`.
 *
 * @typedef {object} RailGateInputs
 * @property {unknown} phase             A key of {@link RAIL_GATE_PHASES}.
 * @property {unknown} meshCount         `engine_state.meshes.length`.
 * @property {unknown} valueCount        `engine_state.values.length`.
 * @property {unknown} constraintCount   `engine_state.constraints.length`.
 * @property {unknown} dispatchCount     `demand_dispatch.dispatch_by_realization` entry count.
 * @property {unknown} activeFile        `store_state.editor.activeFile`.
 * @property {unknown} source            `reify_open_file.source`.
 * @property {unknown} cells             Cell id → {@link RailGateCellReading}.
 * @property {unknown} railSpanPinStatus Folded status of {@link RAIL_SPAN_PIN}.
 * @property {unknown} toolDockPinStatus Folded status of {@link TOOL_DOCK_PIN}.
 * @property {unknown} [requires]           Names of {@link REQUIRABLE_ROWS} this run promises
 *                                          to exercise; a promised row with no reading fails.
 * @property {unknown} [sourceCanonical]    B2 reading — see {@link checkSourceCanonical}.
 * @property {unknown} [fieldCoverage]      B3 reading — see {@link checkFieldCoverage}.
 * @property {unknown} [idempotentReload]   B5 reading — see {@link checkIdempotentReload}.
 * @property {unknown} [rejectionAtomicity] B7 reading, or a list of them — see
 *                                          {@link checkRejectionAtomicity}.
 * @property {unknown} [readOrder]          Harness marker, not a PRD row — parked by
 *                                          {@link observeThenExtras} when the phase's
 *                                          reads were not taken in order.
 *
 * @typedef {object} RailGateResult
 * @property {boolean} ok                 True only when every gate passed.
 * @property {RailGateFailure[]} failures Every violation observed, in gate order.
 */

/** Objects only — an array is not a payload, and neither is `null`. */
function asObject(v) {
  return v !== null && typeof v === "object" && !Array.isArray(v) ? /** @type {any} */ (v) : null;
}

function isCount(v) {
  return typeof v === "number" && Number.isInteger(v) && v >= 0;
}

function isFiniteNumber(v) {
  return typeof v === "number" && Number.isFinite(v);
}

/**
 * Evaluate every gate against one observed state.
 *
 * ALL gates are evaluated and every violation accumulated — this never
 * early-returns — so one failing live run reports the complete picture instead
 * of forcing a re-run per problem. An unknown `phase` suppresses only the
 * phase-DEPENDENT gates (value, constraint), because there is no expectation to
 * compare against; the rest still report.
 *
 * Never throws, for ANY argument: `null` is exactly what a caller holds after a
 * failed read, and a `= {}` parameter default fires only on `undefined`.
 *
 * @param {RailGateInputs} inputs
 * @returns {RailGateResult}
 */
export function checkRailLengtheningGate(inputs) {
  const src = asObject(inputs) ?? {};

  /** @type {RailGateFailure[]} */
  const failures = [];

  // ── Gate 1: the phase itself ────────────────────────────────────────────
  const phase = Object.prototype.hasOwnProperty.call(RAIL_GATE_PHASES, src.phase)
    ? RAIL_GATE_PHASES[/** @type {string} */ (src.phase)]
    : undefined;
  if (phase === undefined) {
    failures.push(shapeFailure("railLengtheningGate", "phase", src.phase, KNOWN_PHASE));
  }

  // ── Gate 2: vacuity ─────────────────────────────────────────────────────
  // Checked whatever the phase: an empty scene proves nothing at any of them.
  for (const [tool, field, observed, floor] of /** @type {const} */ ([
    ["engine_state", "meshes.length", src.meshCount, RAIL_GATE_MIN_BODIES],
    ["engine_state", "values.length", src.valueCount, RAIL_GATE_MIN_VALUES],
    ["engine_state", "constraints.length", src.constraintCount, RAIL_GATE_MIN_CONSTRAINTS],
    ["demand_dispatch", "dispatch_by_realization", src.dispatchCount, RAIL_GATE_MIN_DISPATCHES],
  ])) {
    if (!isCount(observed) || observed < floor) {
      failures.push({ gate: "vacuity", tool, field, observed, expected: floor });
    }
  }

  // ── Gate 3: subject ─────────────────────────────────────────────────────
  // Only the basename is stable; see SUBJECT_BASENAME.
  const activeFile = src.activeFile;
  const onSubject =
    typeof activeFile === "string" &&
    (activeFile === SUBJECT_BASENAME || activeFile.endsWith(`/${SUBJECT_BASENAME}`));
  if (!onSubject) {
    failures.push({
      gate: "subject",
      tool: "store_state",
      field: "editor.activeFile",
      observed: activeFile,
      expected: SUBJECT_BASENAME,
    });
  }

  // ── Gate 4: the source the engine handed back ───────────────────────────
  if (typeof src.source !== "string" || src.source.length === 0) {
    failures.push(shapeFailure("reify_open_file", "source", src.source, NON_EMPTY_STRING));
  }

  // ── Gates 5-7: the three tracked cells ──────────────────────────────────
  const cells = asObject(src.cells) ?? {};
  for (const cell of TRACKED_CELLS) {
    const reading = asObject(cells[cell]);
    if (reading === null) {
      // An absent cell is ONE problem, reported once. Reading it as zero, or
      // additionally faulting its missing freshness, would bury the cause.
      failures.push(shapeFailure("engine_state", `values[${cell}].mm`, cells[cell], NUMBER));
      continue;
    }
    if (!isFiniteNumber(reading.mm)) {
      failures.push(shapeFailure("engine_state", `values[${cell}].mm`, reading.mm, NUMBER));
    }
    if (reading.freshness !== LIVE_FRESHNESS) {
      failures.push({
        gate: "stale",
        tool: "engine_state",
        field: `values[${cell}].freshness`,
        observed: reading.freshness,
        expected: LIVE_FRESHNESS,
      });
    }
    if (phase !== undefined && isFiniteNumber(reading.mm)) {
      const want = phase.cells[cell];
      if (Math.abs(/** @type {number} */ (reading.mm) - want) > RAIL_GATE_LENGTH_TOLERANCE_MM) {
        failures.push({
          gate: "value",
          tool: "engine_state",
          field: `values[${cell}].mm`,
          observed: reading.mm,
          expected: want,
        });
      }
    }
  }

  // ── Gate 8: the two named pins ──────────────────────────────────────────
  // NEVER a global "no constraints violated": printer.ri carries eleven
  // indeterminate constraints at baseline, and the last phase legitimately
  // leaves the ToolDock pin red. Both would red a global assertion on correct
  // behaviour; see this module's header.
  if (phase !== undefined) {
    for (const [pin, observed, want] of /** @type {const} */ ([
      [RAIL_SPAN_PIN, src.railSpanPinStatus, phase.railSpanPinStatus],
      [TOOL_DOCK_PIN, src.toolDockPinStatus, phase.toolDockPinStatus],
    ])) {
      if (observed !== want) {
        failures.push({
          gate: "constraint",
          tool: "engine_state",
          field: `constraints[${pin}].status`,
          observed,
          expected: want,
        });
      }
    }
  }

  // ── Gate 9: the remaining PRD rows ──────────────────────────────────────
  // Supplying a reading is what asks for it to be graded. `requires` is the
  // other direction and exists only to close the silent-skip hole: a driver
  // that MEANT to exercise a row and passed nothing would otherwise be graded
  // as a pass on a row it never ran — the same vacuity trap gate 2 closes for
  // an empty scene.
  // A present-but-non-array `requires` is faulted rather than discarded: reading
  // it as "promised nothing" would reopen the very silent-skip hole the field
  // exists to close, and one misspelled container would disarm every promise the
  // run meant to make.
  if (src.requires !== undefined && !Array.isArray(src.requires)) {
    failures.push(shapeFailure("railLengtheningGate", "requires", src.requires, ARRAY));
  }
  // A name is either a PROMISE or a MISTAKE, never both: a rejected name must
  // not also raise the "you promised this and supplied nothing" vacuity below,
  // which would report one error twice under two different gates.
  const promised = [];
  for (const name of Array.isArray(src.requires) ? src.requires : []) {
    if (REQUIRABLE_ROWS.includes(name)) promised.push(name);
    else failures.push(shapeFailure("railLengtheningGate", "requires", name, KNOWN_EXTRA));
  }
  for (const [name, { check, list }] of Object.entries(EXTRA_GATES)) {
    const reading = src[name];
    if (reading === undefined) {
      if (promised.includes(name)) {
        failures.push({
          gate: "vacuity",
          tool: "railLengtheningGate",
          field: name,
          observed: undefined,
          expected: "a reading to grade",
        });
      }
      continue;
    }
    for (const one of list && Array.isArray(reading) ? reading : [reading]) {
      failures.push(...check(one));
    }
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
 * @param {RailGateFailure[]} failures  Accumulator, mutated in place.
 * @returns {Record<string, unknown> | null} The payload, or null if unusable.
 */
function usablePayload(payload, toolName, failures) {
  if (isInBandError(payload)) {
    // `outage`, NOT `shape`: the tool FAILED. A caller must be able to tell that
    // apart from a wrong-shaped answer, because it means the invariant was never
    // tested at all.
    failures.push({ gate: "outage", tool: toolName, observed: /** @type {any} */ (payload).error });
    return null;
  }
  const obj = asObject(payload);
  if (obj === null) {
    failures.push(shapeFailure(toolName, undefined, payload, OBJECT));
    return null;
  }
  return obj;
}

/**
 * Read one array field off a payload, faulting it by name when it is not one.
 *
 * @returns {any[] | undefined}
 */
function arrayField(payload, toolName, field, failures) {
  if (payload === null) return undefined;
  const v = payload[field];
  if (!Array.isArray(v)) {
    failures.push(shapeFailure(toolName, field, v, ARRAY));
    return undefined;
  }
  return v;
}

/**
 * Millimetre reading for one cell, from the `engine_state` `values` list.
 *
 * `si_value` (canonical SI, metres) is the number read — NOT the formatted
 * `value`/`unit` pair, which would need a parser and would silently change
 * meaning when the unit picker changes. A cell that is missing, or that carries
 * no usable `si_value`, yields `undefined`, which
 * {@link checkRailLengtheningGate} reports as a named shape failure — this
 * function faults nothing itself, so extraction reports payload problems and the
 * check reports cell problems, each in one place.
 *
 * @param {any[] | undefined} values
 * @param {string} cell
 * @returns {{mm: number|undefined, freshness: unknown} | undefined}
 */
function cellReading(values, cell) {
  if (!Array.isArray(values)) return undefined;
  const entry = values.find(
    (v) => v !== null && typeof v === "object" && /** @type {any} */ (v).cell_id === cell,
  );
  if (entry === undefined) return undefined;
  const si = /** @type {any} */ (entry).si_value;
  return {
    mm: isFiniteNumber(si) ? /** @type {number} */ (si) * 1000 : undefined,
    freshness: /** @type {any} */ (entry).freshness,
  };
}

/**
 * @typedef {object} RailGateExtraction
 * @property {RailGateInputs} inputs   Flat shape accepted by {@link checkRailLengtheningGate};
 *                                     a field is `undefined` exactly when its extraction failed.
 * @property {RailGateFailure[]} failures Per-tool extraction problems — `outage` when the tool
 *                                     itself failed, `shape` when it answered unreadably.
 */

/**
 * Flatten the four live debug-MCP payloads into the checker's input shape.
 *
 * Shapes read (note the casing seam — the frontend speaks camelCase, Rust speaks
 * snake_case):
 *   `engine_state`    → `{meshes: [...], values: [{cell_id, si_value, freshness, …}],
 *                         constraints: [{node_id, status, parameter_ids, …}], …}`
 *   `store_state`     → `{editor: {activeFile, dirtyFiles, openFiles}, …}`
 *   `reify_open_file` → `{success, source}`  (debug_server.rs reify_open_file_envelope)
 *   `demand_dispatch` → `{dispatch_by_realization, eval_set, full_scope}`
 *
 * An extraction failure means the invariant was NEVER TESTED — an outage, not a
 * pass. The undefined inputs it leaves behind are what make the verdict fail
 * too, so a caller cannot accidentally read silence as success.
 *
 * Never throws, for ANY argument, including `null`.
 *
 * @param {{phase?: unknown, engineState?: unknown, storeState?: unknown,
 *          openFile?: unknown, demandDispatch?: unknown}} payloads
 * @returns {RailGateExtraction}
 */
export function extractGateInputs(payloads) {
  const src = asObject(payloads) ?? {};

  /** @type {RailGateFailure[]} */
  const failures = [];

  const engine = usablePayload(src.engineState, "engine_state", failures);
  const meshes = arrayField(engine, "engine_state", "meshes", failures);
  const values = arrayField(engine, "engine_state", "values", failures);
  const constraints = arrayField(engine, "engine_state", "constraints", failures);

  const store = usablePayload(src.storeState, "store_state", failures);
  const editor = store === null ? null : asObject(store["editor"]);

  const opened = usablePayload(src.openFile, "reify_open_file", failures);
  let source;
  if (opened !== null) {
    const raw = opened["source"];
    if (typeof raw !== "string" || raw.length === 0) {
      failures.push(shapeFailure("reify_open_file", "source", raw, NON_EMPTY_STRING));
    } else {
      source = raw;
    }
  }

  const dispatch = usablePayload(src.demandDispatch, "demand_dispatch", failures);
  let dispatchCount;
  if (dispatch !== null) {
    const byRealization = asObject(dispatch["dispatch_by_realization"]);
    if (byRealization === null) {
      failures.push(
        shapeFailure("demand_dispatch", "dispatch_by_realization", dispatch["dispatch_by_realization"], OBJECT),
      );
    } else {
      dispatchCount = Object.keys(byRealization).length;
    }
  }

  /** @type {Record<string, unknown>} */
  const cells = {};
  for (const cell of TRACKED_CELLS) {
    const reading = cellReading(values, cell);
    if (reading !== undefined) cells[cell] = reading;
  }

  return {
    inputs: {
      phase: src.phase,
      meshCount: meshes === undefined ? undefined : meshes.length,
      valueCount: values === undefined ? undefined : values.length,
      constraintCount: constraints === undefined ? undefined : constraints.length,
      dispatchCount,
      activeFile: editor === null ? undefined : editor["activeFile"],
      source,
      cells,
      railSpanPinStatus: foldPinStatus(
        selectPinConstraints(constraints, PIN_SELECTORS[RAIL_SPAN_PIN]),
      ),
      toolDockPinStatus: foldPinStatus(
        selectPinConstraints(constraints, PIN_SELECTORS[TOOL_DOCK_PIN]),
      ),
    },
    failures,
  };
}
