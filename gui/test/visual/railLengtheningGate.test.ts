/**
 * CI-gated backstop for the printer_v01 Y-rail lengthening integration gate
 * (task 5098; PRD docs/prds/v0_6/ai-native-editing.md §7 leaf ζ).
 *
 * The live half of that gate (`./smoke_rail_lengthening_e2e.mjs`) needs a real
 * webview, a real OCCT and a real `reify_set_parameter` round trip, so it can
 * never run in CI. This suite pins the part that can actually regress — the
 * decision function that turns four debug-MCP payloads into a pass/fail
 * verdict — as pure data, with no RPC and no GUI.
 *
 * THE SCENARIO, as measured on printer.ri (the truth table `RAIL_GATE_PHASES`
 * encodes; the reading was taken with `reify check`, and the same three states
 * are re-derived by the live driver through the GUI):
 *
 *   baseline          y_rail_len 800  rail_span_m 800   travel_avail 510
 *   after-y-rail      y_rail_len 1100 rail_span_m 800   travel_avail 510
 *   after-rail-span   y_rail_len 1100 rail_span_m 1100  travel_avail 810
 *
 * TWO THINGS THIS SUITE DELIBERATELY DOES NOT ASSERT, both because the measured
 * behaviour contradicts the obvious reading:
 *
 *   * "the pins go green again" is NOT a zero-violations claim. Lengthening the
 *     rail span moves AFrame.travel_avail 510 -> 810, which drags the ToolDock
 *     pinned literal `yh_min_today` off its `centre_y - travel_avail/2` target,
 *     so a SECOND pin legitimately goes red at `after-rail-span`. A
 *     zero-violations assertion would red on correct behaviour. The gate names
 *     the two pins it is about and asserts each one's own expected status —
 *     which makes the ToolDock flip a second, free constraint-status-sync
 *     signal rather than a nuisance.
 *   * a pin is never selected by its `node_id` index. `Printer#constraint[45]`
 *     is positional: adding a constraint anywhere above it in printer.ri would
 *     silently retarget an index-keyed assertion at a different predicate. The
 *     selector is a superset match on `parameter_ids` (which is
 *     `collect_value_refs(expr)`, engine.rs), so the pin is named by what it is
 *     about.
 *
 * ASSERT ON RECORDS, NOT PROSE — the house rule from `./meshCountParity.test.ts`.
 * Every gate returns a {gate, tool, field, observed, expected} record, so the
 * cases below check the observation itself rather than substring-matching an
 * English sentence. The rendered wording is pinned once, in the formatFailures
 * block at the end.
 */
import { describe, it, expect } from "vitest";
import {
  RAIL_GATE_MIN_BODIES,
  RAIL_GATE_MIN_CONSTRAINTS,
  RAIL_GATE_MIN_DISPATCHES,
  RAIL_GATE_MIN_VALUES,
  RAIL_GATE_PHASES,
  RAIL_SPAN_CELL,
  RAIL_SPAN_PIN,
  SUBJECT_BASENAME,
  TOOL_DOCK_PIN,
  TRAVEL_AVAIL_CELL,
  Y_RAIL_LEN_CELL,
  checkRailLengtheningGate,
  extractGateInputs,
  formatFailures,
} from "./railLengtheningGate.mjs";

type Failure = {
  gate: string;
  tool: string;
  field?: string;
  observed: unknown;
  expected?: unknown;
};

type Verdict = { ok: boolean; failures: Failure[] };
type Extraction = { inputs: Record<string, unknown>; failures: Failure[] };

const SUBJECT_PATH = "/tmp/rail-gate-abc123/printer_v01/printer.ri";

/** One `engine_state` `values[]` entry, in the shape the live payload carries. */
function value(cellId: string, mm: number, freshness = "final") {
  const [entity, name] = cellId.split(".");
  return {
    cell_id: cellId,
    name,
    value: String(mm),
    unit: "mm",
    determinacy: "determined",
    entity_path: entity,
    kind: "Param",
    freshness,
    reason: null,
    last_substantive_value: null,
    si_value: mm / 1000,
    dimension: "Length",
  };
}

/** One `engine_state` `constraints[]` entry. */
function constraint(nodeId: string, status: string, parameterIds: string[]) {
  return {
    node_id: nodeId,
    expression: `<${nodeId}>`,
    status,
    label: null,
    parameter_ids: [...parameterIds].sort(),
  };
}

/**
 * The rail-span pin is a PAIR — printer.ri writes `x < y + slack` and
 * `x > y - slack` — and both halves carry the same `parameter_ids`, so the
 * selector matches both. `railSpanStatus` sets the `>` half, which is the one
 * that actually flips.
 */
function constraintsFor(railSpanStatus: string, toolDockStatus: string) {
  const railCells = [RAIL_SPAN_CELL, Y_RAIL_LEN_CELL, "Printer.o1_pin_slack"];
  const dockCells = [
    "AFrame.centre_y",
    TRAVEL_AVAIL_CELL,
    "Printer.o1_pin_slack",
    "ToolDock.yh_min_today",
  ];
  return [
    constraint("Printer#constraint[44]", "Satisfied", railCells),
    constraint("Printer#constraint[45]", railSpanStatus, railCells),
    constraint("Printer#constraint[88]", toolDockStatus, dockCells),
    constraint("Printer#constraint[89]", "Satisfied", dockCells),
    // An unrelated pin that must never be selected: it names ONE of the two
    // rail cells, so a match-any selector would pick it up.
    constraint("Printer#constraint[7]", "Satisfied", [RAIL_SPAN_CELL, "AFrame.brg_len_m"]),
  ];
}

/** A complete, passing input for one phase — the shape the cases below perturb. */
function inputsFor(
  phase: string,
  yRailLenMm: number,
  railSpanMm: number,
  travelAvailMm: number,
  railSpanPinStatus: string,
  toolDockPinStatus: string,
) {
  return {
    phase,
    meshCount: 240,
    valueCount: 900,
    constraintCount: 120,
    dispatchCount: 40,
    activeFile: SUBJECT_PATH,
    source: "param y_rail_len : Length = 800mm\n",
    cells: {
      [Y_RAIL_LEN_CELL]: { mm: yRailLenMm, freshness: "final" },
      [RAIL_SPAN_CELL]: { mm: railSpanMm, freshness: "final" },
      [TRAVEL_AVAIL_CELL]: { mm: travelAvailMm, freshness: "final" },
    },
    railSpanPinStatus,
    toolDockPinStatus,
  };
}

const BASELINE = inputsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
const AFTER_Y_RAIL = inputsFor("after-y-rail", 1100, 800, 510, "Violated", "Satisfied");
const AFTER_RAIL_SPAN = inputsFor("after-rail-span", 1100, 1100, 810, "Satisfied", "Violated");

/** Every failure record naming one field. */
const forField = (failures: Failure[], field: string) => failures.filter((f) => f.field === field);
/** Every failure record from one gate. */
const forGate = (failures: Failure[], gate: string) => failures.filter((f) => f.gate === gate);

// ─────────────────────────────────────────────────────────────────────────────

describe("RAIL_GATE_PHASES — the measured truth table", () => {
  it("names exactly the three states the scenario passes through", () => {
    expect(Object.keys(RAIL_GATE_PHASES)).toEqual([
      "baseline",
      "after-y-rail",
      "after-rail-span",
    ]);
  });

  it("pins the two edits and the derived travel each phase expects", () => {
    expect(RAIL_GATE_PHASES["baseline"]).toMatchObject({
      cells: { [Y_RAIL_LEN_CELL]: 800, [RAIL_SPAN_CELL]: 800, [TRAVEL_AVAIL_CELL]: 510 },
    });
    expect(RAIL_GATE_PHASES["after-y-rail"]).toMatchObject({
      cells: { [Y_RAIL_LEN_CELL]: 1100, [RAIL_SPAN_CELL]: 800, [TRAVEL_AVAIL_CELL]: 510 },
    });
    expect(RAIL_GATE_PHASES["after-rail-span"]).toMatchObject({
      cells: { [Y_RAIL_LEN_CELL]: 1100, [RAIL_SPAN_CELL]: 1100, [TRAVEL_AVAIL_CELL]: 810 },
    });
  });

  it("encodes the rail-span pin's Satisfied -> Violated -> Satisfied transition", () => {
    expect([
      RAIL_GATE_PHASES["baseline"]!.railSpanPinStatus,
      RAIL_GATE_PHASES["after-y-rail"]!.railSpanPinStatus,
      RAIL_GATE_PHASES["after-rail-span"]!.railSpanPinStatus,
    ]).toEqual(["Satisfied", "Violated", "Satisfied"]);
  });

  it("encodes the ToolDock cascade as EXPECTED, not as a failure to be tolerated", () => {
    // Lengthening the rail span moves travel_avail 510 -> 810, which puts the
    // ToolDock's pinned `yh_min_today` literal out of range. The gate demands
    // that flip; it does not merely permit it.
    expect([
      RAIL_GATE_PHASES["baseline"]!.toolDockPinStatus,
      RAIL_GATE_PHASES["after-y-rail"]!.toolDockPinStatus,
      RAIL_GATE_PHASES["after-rail-span"]!.toolDockPinStatus,
    ]).toEqual(["Satisfied", "Satisfied", "Violated"]);
  });
});

describe("checkRailLengtheningGate — (a) the three measured states pass", () => {
  it.each([
    ["baseline", BASELINE],
    ["after-y-rail", AFTER_Y_RAIL],
    ["after-rail-span", AFTER_RAIL_SPAN],
  ])("passes at %s", (_name, inputs) => {
    expect(checkRailLengtheningGate(inputs)).toEqual({ ok: true, failures: [] });
  });
});

describe("checkRailLengtheningGate — (b) the value gate", () => {
  it("reads travel_avail 510 at baseline and rejects the post-edit number there", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [TRAVEL_AVAIL_CELL]: { mm: 810, freshness: "final" } },
    }) as Verdict;
    expect(ok).toBe(false);
    expect(forField(failures, `values[${TRAVEL_AVAIL_CELL}].mm`)).toEqual([
      {
        gate: "value",
        tool: "engine_state",
        field: `values[${TRAVEL_AVAIL_CELL}].mm`,
        observed: 810,
        expected: 510,
      },
    ]);
  });

  it("reads travel_avail 810 after the rail-span edit and rejects the stale number", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      cells: { ...AFTER_RAIL_SPAN.cells, [TRAVEL_AVAIL_CELL]: { mm: 510, freshness: "final" } },
    }) as Verdict;
    expect(forField(failures, `values[${TRAVEL_AVAIL_CELL}].mm`)).toEqual([
      {
        gate: "value",
        tool: "engine_state",
        field: `values[${TRAVEL_AVAIL_CELL}].mm`,
        observed: 510,
        expected: 810,
      },
    ]);
  });

  it("localises which of the three cells drifted", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_Y_RAIL,
      cells: { ...AFTER_Y_RAIL.cells, [Y_RAIL_LEN_CELL]: { mm: 800, freshness: "final" } },
    }) as Verdict;
    expect(failures.map((f) => f.field)).toEqual([`values[${Y_RAIL_LEN_CELL}].mm`]);
  });

  it("tolerates float noise in the SI round trip", () => {
    // si_value arrives in metres and is scaled by 1000, so 0.51 m can land a few
    // ulps off 510. An exact === here would red on a correct run.
    const { ok } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [TRAVEL_AVAIL_CELL]: { mm: 510 + 1e-9, freshness: "final" } },
    }) as Verdict;
    expect(ok).toBe(true);
  });

  it("names a cell whose reading is not a finite number as a shape failure, not a value one", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [RAIL_SPAN_CELL]: { mm: undefined, freshness: "final" } },
    }) as Verdict;
    expect(failures).toEqual([
      {
        gate: "shape",
        tool: "engine_state",
        field: `values[${RAIL_SPAN_CELL}].mm`,
        observed: undefined,
        expected: "finite-number",
      },
    ]);
  });

  it("names a missing cell rather than reading it as zero", () => {
    const { failures } = checkRailLengtheningGate({ ...BASELINE, cells: {} }) as Verdict;
    expect(forGate(failures, "shape").map((f) => f.field)).toEqual([
      `values[${Y_RAIL_LEN_CELL}].mm`,
      `values[${RAIL_SPAN_CELL}].mm`,
      `values[${TRAVEL_AVAIL_CELL}].mm`,
    ]);
  });
});

describe("checkRailLengtheningGate — (c) the staleness gate", () => {
  it("refuses a pruned cell whose number is the last good value, not the live one", () => {
    // A demand-pruned cell reads freshness 'pending' and keeps its previous
    // value, so the value gate would PASS on a number the edit never produced.
    const { ok, failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      cells: { ...AFTER_RAIL_SPAN.cells, [TRAVEL_AVAIL_CELL]: { mm: 810, freshness: "pending" } },
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "stale",
        tool: "engine_state",
        field: `values[${TRAVEL_AVAIL_CELL}].freshness`,
        observed: "pending",
        expected: "final",
      },
    ]);
  });

  it("refuses a failed cell too", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [Y_RAIL_LEN_CELL]: { mm: 800, freshness: "failed" } },
    }) as Verdict;
    expect(forGate(failures, "stale").map((f) => f.observed)).toEqual(["failed"]);
  });
});

describe("checkRailLengtheningGate — (d) the constraint gate", () => {
  it("demands the rail-span pin flip to Violated after the y_rail_len edit", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...AFTER_Y_RAIL,
      railSpanPinStatus: "Satisfied",
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "constraint",
        tool: "engine_state",
        field: `constraints[${RAIL_SPAN_PIN}].status`,
        observed: "Satisfied",
        expected: "Violated",
      },
    ]);
  });

  it("demands the rail-span pin return to Satisfied after the rail_span_m edit", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      railSpanPinStatus: "Violated",
    }) as Verdict;
    expect(forField(failures, `constraints[${RAIL_SPAN_PIN}].status`)).toEqual([
      {
        gate: "constraint",
        tool: "engine_state",
        field: `constraints[${RAIL_SPAN_PIN}].status`,
        observed: "Violated",
        expected: "Satisfied",
      },
    ]);
  });

  it("demands the ToolDock cascade, so a pin that stays green is a FAILURE", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      toolDockPinStatus: "Satisfied",
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "constraint",
        tool: "engine_state",
        field: `constraints[${TOOL_DOCK_PIN}].status`,
        observed: "Satisfied",
        expected: "Violated",
      },
    ]);
  });

  it("reports an absent pin as a constraint failure naming 'absent'", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      railSpanPinStatus: "absent",
    }) as Verdict;
    expect(forField(failures, `constraints[${RAIL_SPAN_PIN}].status`)![0]!.observed).toBe("absent");
  });

  it("never asserts a global 'no constraints violated' — only the two named pins", () => {
    // The scenario legitimately leaves a violated pin behind at the last phase,
    // and printer.ri carries eleven indeterminate constraints at baseline. Any
    // gate keyed on a global count would red on correct behaviour.
    const { ok } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      constraintCount: 120,
    }) as Verdict;
    expect(ok).toBe(true);
  });
});

describe("checkRailLengtheningGate — (e) the vacuity gate", () => {
  it("rejects an empty scene: nothing loaded means the invariant was never tested", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...BASELINE,
      meshCount: 0,
      valueCount: 0,
      constraintCount: 0,
      dispatchCount: 0,
    }) as Verdict;
    expect(ok).toBe(false);
    expect(forGate(failures, "vacuity").map((f) => [f.field, f.observed, f.expected])).toEqual([
      ["meshes.length", 0, RAIL_GATE_MIN_BODIES],
      ["values.length", 0, RAIL_GATE_MIN_VALUES],
      ["constraints.length", 0, RAIL_GATE_MIN_CONSTRAINTS],
      ["dispatch_by_realization", 0, RAIL_GATE_MIN_DISPATCHES],
    ]);
  });

  it("rejects a PARTIAL load, not just a total one", () => {
    const { ok } = checkRailLengtheningGate({ ...BASELINE, meshCount: 3 }) as Verdict;
    expect(ok).toBe(false);
  });

  it("names demand_dispatch as the tool for the dispatch floor", () => {
    const { failures } = checkRailLengtheningGate({ ...BASELINE, dispatchCount: 0 }) as Verdict;
    expect(failures.map((f) => f.tool)).toEqual(["demand_dispatch"]);
  });
});

describe("checkRailLengtheningGate — (f) the subject gate", () => {
  it("rejects a run that graded some other file", () => {
    const { ok, failures } = checkRailLengtheningGate({
      ...BASELINE,
      activeFile: "/home/leo/src/reify/gui/test/fixtures/large_assembly.ri",
    }) as Verdict;
    expect(ok).toBe(false);
    expect(failures).toEqual([
      {
        gate: "subject",
        tool: "store_state",
        field: "editor.activeFile",
        observed: "/home/leo/src/reify/gui/test/fixtures/large_assembly.ri",
        expected: SUBJECT_BASENAME,
      },
    ]);
  });

  it("accepts the temp-directory copy the live driver actually opens", () => {
    // The driver never mutates the tracked design file; it drives a mkdtemp
    // copy, and canonicalize resolves /tmp's symlink, so only the basename is
    // stable across the round trip.
    expect(checkRailLengtheningGate({ ...BASELINE, activeFile: SUBJECT_PATH }).ok).toBe(true);
  });
});

describe("checkRailLengtheningGate — (g) never throws, for ANY argument", () => {
  it.each([
    ["null", null],
    ["undefined", undefined],
    ["a string", "engine_state"],
    ["a number", 7],
    ["an array", []],
    ["an in-band error envelope", { error: "engine not ready" }],
    ["an inputs object with a null cells map", { ...BASELINE, cells: null }],
  ])("returns a verdict for %s rather than throwing", (_name, arg) => {
    const verdict = checkRailLengtheningGate(arg) as Verdict;
    expect(verdict.ok).toBe(false);
    expect(Array.isArray(verdict.failures)).toBe(true);
    expect(verdict.failures.length).toBeGreaterThan(0);
  });

  it("names an unknown phase as a shape failure and still grades the rest", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      phase: "after-the-third-edit",
      meshCount: 0,
    }) as Verdict;
    expect(forGate(failures, "shape")).toEqual([
      {
        gate: "shape",
        tool: "railLengtheningGate",
        field: "phase",
        observed: "after-the-third-edit",
        expected: "one-of-RAIL_GATE_PHASES",
      },
    ]);
    // The phase-independent gates are still evaluated — a bad phase must not
    // swallow the rest of the diagnosis.
    expect(forGate(failures, "vacuity").length).toBeGreaterThan(0);
  });
});

// ─── extractGateInputs ───────────────────────────────────────────────────────

/** A complete, healthy payload set for one phase. */
function payloadsFor(
  phase: string,
  yRailLenMm: number,
  railSpanMm: number,
  travelAvailMm: number,
  railSpanStatus: string,
  toolDockStatus: string,
) {
  return {
    phase,
    engineState: {
      meshes: new Array(240).fill({ entity_path: "Printer#realization[0]" }),
      values: [
        value(Y_RAIL_LEN_CELL, yRailLenMm),
        value(RAIL_SPAN_CELL, railSpanMm),
        value(TRAVEL_AVAIL_CELL, travelAvailMm),
        value("ToolDock.yh_min_today", 145),
      ],
      constraints: constraintsFor(railSpanStatus, toolDockStatus),
    },
    storeState: { editor: { activeFile: SUBJECT_PATH, dirtyFiles: [], openFiles: [] } },
    openFile: { success: true, source: "param y_rail_len : Length = 800mm\n" },
    demandDispatch: {
      dispatch_by_realization: Object.fromEntries(
        new Array(40).fill(0).map((_, i) => [`Printer#realization[${i}]`, 1]),
      ),
      eval_set: [],
      full_scope: false,
    },
  };
}

describe("extractGateInputs — (h) folding live payloads into flat scalars", () => {
  it("extracts a complete, gradeable baseline with no failures", () => {
    const { inputs, failures } = extractGateInputs(
      payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied"),
    ) as Extraction;
    expect(failures).toEqual([]);
    expect(checkRailLengtheningGate(inputs)).toEqual({ ok: true, failures: [] });
  });

  it("scales si_value from metres to millimetres", () => {
    const { inputs } = extractGateInputs(
      payloadsFor("after-rail-span", 1100, 1100, 810, "Satisfied", "Violated"),
    ) as Extraction;
    expect((inputs["cells"] as Record<string, { mm: number }>)[TRAVEL_AVAIL_CELL]!.mm).toBeCloseTo(
      810,
      9,
    );
  });

  it("selects the pin by parameter_ids superset, never by node_id index", () => {
    const payloads = payloadsFor("after-y-rail", 1100, 800, 510, "Violated", "Satisfied");
    // Renumber every constraint. An index-keyed selector would now miss.
    payloads.engineState.constraints = payloads.engineState.constraints.map((c, i) => ({
      ...c,
      node_id: `Printer#constraint[${900 + i}]`,
    }));
    const { inputs, failures } = extractGateInputs(payloads) as Extraction;
    expect(failures).toEqual([]);
    expect(inputs["railSpanPinStatus"]).toBe("Violated");
  });

  it("folds the two halves of a pin pair: Violated on either half is Violated", () => {
    const { inputs } = extractGateInputs(
      payloadsFor("after-y-rail", 1100, 800, 510, "Violated", "Satisfied"),
    ) as Extraction;
    // Only `Printer#constraint[45]` is Violated; `[44]` shares the same
    // parameter_ids and is Satisfied.
    expect(inputs["railSpanPinStatus"]).toBe("Violated");
    expect(inputs["toolDockPinStatus"]).toBe("Satisfied");
  });

  it("folds an Indeterminate half as Indeterminate when no half is Violated", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Indeterminate", "Satisfied");
    const { inputs } = extractGateInputs(payloads) as Extraction;
    expect(inputs["railSpanPinStatus"]).toBe("Indeterminate");
  });

  it("reports an unmatched pin as 'absent' rather than silently passing", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    payloads.engineState.constraints = payloads.engineState.constraints.filter(
      (c) => !c.parameter_ids.includes(Y_RAIL_LEN_CELL),
    );
    const { inputs } = extractGateInputs(payloads) as Extraction;
    expect(inputs["railSpanPinStatus"]).toBe("absent");
  });

  it("classifies an in-band tool error as an OUTAGE, not a shape problem", () => {
    // An outage means the invariant was never tested; a caller must be able to
    // tell that apart from a gate that was tested and violated.
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { failures } = extractGateInputs({
      ...payloads,
      engineState: { error: "engine session poisoned" },
    }) as Extraction;
    expect(forGate(failures, "outage")).toEqual([
      { gate: "outage", tool: "engine_state", observed: "engine session poisoned" },
    ]);
  });

  it("names each unreadable payload rather than reporting one blanket problem", () => {
    const { failures } = extractGateInputs({
      phase: "baseline",
      engineState: null,
      storeState: 42,
      openFile: [],
      demandDispatch: "nope",
    }) as Extraction;
    expect(forGate(failures, "shape").map((f) => f.tool)).toEqual([
      "engine_state",
      "store_state",
      "reify_open_file",
      "demand_dispatch",
    ]);
  });

  it("flags a non-array meshes/values/constraints field by name", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { failures } = extractGateInputs({
      ...payloads,
      engineState: { meshes: 240, values: null, constraints: "many" },
    }) as Extraction;
    expect(forGate(failures, "shape").map((f) => [f.field, f.expected])).toEqual([
      ["meshes", "array"],
      ["values", "array"],
      ["constraints", "array"],
    ]);
  });

  it("flags a missing reify_open_file source", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { failures } = extractGateInputs({
      ...payloads,
      openFile: { success: true },
    }) as Extraction;
    expect(forGate(failures, "shape").map((f) => [f.tool, f.field, f.expected])).toEqual([
      ["reify_open_file", "source", "non-empty-string"],
    ]);
  });

  it("never throws for a null argument — the shape a caller holds after a failed read", () => {
    const { inputs, failures } = extractGateInputs(null) as Extraction;
    expect(failures.length).toBeGreaterThan(0);
    expect(checkRailLengtheningGate(inputs).ok).toBe(false);
  });

  it("an extraction failure means the invariant was NEVER TESTED, so the verdict is not ok", () => {
    const payloads = payloadsFor("baseline", 800, 800, 510, "Satisfied", "Satisfied");
    const { inputs, failures } = extractGateInputs({
      ...payloads,
      engineState: { error: "boom" },
    }) as Extraction;
    expect(failures.length).toBeGreaterThan(0);
    expect(checkRailLengtheningGate(inputs).ok).toBe(false);
  });
});

// ─── formatFailures ──────────────────────────────────────────────────────────

describe("formatFailures — the single site where a record becomes English", () => {
  it("renders one line per failure, in the order given", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_RAIL_SPAN,
      meshCount: 0,
      railSpanPinStatus: "Violated",
    }) as Verdict;
    expect(formatFailures(failures)).toHaveLength(failures.length);
  });

  it("explains a constraint failure in terms of the pin, the phase and both statuses", () => {
    const { failures } = checkRailLengtheningGate({
      ...AFTER_Y_RAIL,
      railSpanPinStatus: "Satisfied",
    }) as Verdict;
    const line = formatFailures(failures)[0]!;
    expect(line).toContain(RAIL_SPAN_PIN);
    expect(line).toContain("Satisfied");
    expect(line).toContain("Violated");
  });

  it("explains a stale reading by naming demand pruning, the cause that produces it", () => {
    const { failures } = checkRailLengtheningGate({
      ...BASELINE,
      cells: { ...BASELINE.cells, [TRAVEL_AVAIL_CELL]: { mm: 510, freshness: "pending" } },
    }) as Verdict;
    expect(formatFailures(failures)[0]!).toMatch(/prun/i);
  });

  it("says an outage means the invariant was never tested", () => {
    const line = formatFailures([
      { gate: "outage", tool: "engine_state", observed: "boom" },
    ] as Failure[])[0]!;
    expect(line).toMatch(/never/i);
  });

  it("degrades a malformed record to a dump rather than throwing", () => {
    expect(formatFailures([null, 7, { gate: "no-such-gate", tool: "x", observed: 1 }] as never)).
      toHaveLength(3);
  });

  it("returns an empty list for a non-array argument", () => {
    expect(formatFailures(null as never)).toEqual([]);
  });
});
