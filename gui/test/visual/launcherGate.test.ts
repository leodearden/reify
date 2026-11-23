/**
 * The CI half of the launcher smoke (task 9730): every decision the live driver
 * `./smoke_launcher_e2e.mjs` makes, as pure data, for both of its scenarios.
 *
 * Failure records are asserted on their stable `code`, never on message text.
 */
import { describe, it, expect } from "vitest";
import {
  LAUNCHER_GATE_CODES,
  gradeOpenScenario,
  gradeNewFileScenario,
  formatFailures,
} from "./launcherGate.mjs";

const codes = (failures: Array<{ code: string }>) => failures.map((f) => f.code);

const FIXTURE = "/repo/examples/m5_geometry.ri";

const START_VIEW = { loaded: false, startViewVisible: true };

const OPEN_PASSING = {
  startView: START_VIEW,
  debugMode: {
    debugPortEnv: 3939,
    atLaunch: { selected: "debug", debugPort: 3939 },
    first: { selected: "debug", debugPort: 3939 },
    second: { selected: "debug", debugPort: 3939 },
    healthAfter: true,
  },
  failedOpen: { loaded: false, errorCode: "E_GUI_OPEN_FAILED", startViewVisible: true },
  opened: {
    loaded: true,
    activeFile: FIXTURE,
    fixtureName: "m5_geometry.ri",
    meshCount: 2,
    recentFirstPath: FIXTURE,
    startViewVisible: false,
  },
};

const NEW_PASSING = {
  startView: START_VIEW,
  created: { existsOnDisk: true, firstLine: "module launcher_new", stem: "launcher_new" },
  loaded: true,
  activeFile: "/tmp/t/launcher_new.ri",
  compileErrorCount: 0,
  meshCount: 1,
  startViewVisible: false,
};

describe("gradeOpenScenario", () => {
  it("passes a fully green observation", () => {
    expect(gradeOpenScenario(OPEN_PASSING)).toEqual([]);
  });

  it.each([
    ["a loaded document at launch", { loaded: true, startViewVisible: true }],
    ["a hidden start view at launch", { loaded: false, startViewVisible: false }],
  ])("B1: fails %s as START_VIEW_NOT_SHOWN", (_label, startView) => {
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, startView }))).toEqual([
      LAUNCHER_GATE_CODES.START_VIEW_NOT_SHOWN,
    ]);
  });

  it.each([
    ["an env launch not reporting debug", { atLaunch: { selected: "assistant", debugPort: 3939 } }],
    ["an env launch on another port", { atLaunch: { selected: "debug", debugPort: 4000 } }],
    ["Debug selecting another port", { first: { selected: "debug", debugPort: 4000 }, second: { selected: "debug", debugPort: 4000 } }],
    ["Debug not selecting debug", { first: { selected: "design", debugPort: 3939 } }],
  ])("B5: fails %s as DEBUG_PORT_MISMATCH", (_label, change) => {
    const debugMode = { ...OPEN_PASSING.debugMode, ...change };
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, debugMode }))).toContain(
      LAUNCHER_GATE_CODES.DEBUG_PORT_MISMATCH,
    );
  });

  it.each([
    ["a second Debug on another port", { second: { selected: "debug", debugPort: 4001 } }],
    ["a second Debug leaving debug", { second: { selected: "design", debugPort: 3939 } }],
    ["a dead server after re-selecting", { healthAfter: false }],
  ])("B5: fails %s as DEBUG_NOT_IDEMPOTENT", (_label, change) => {
    const debugMode = { ...OPEN_PASSING.debugMode, ...change };
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, debugMode }))).toEqual([
      LAUNCHER_GATE_CODES.DEBUG_NOT_IDEMPOTENT,
    ]);
  });

  it.each([
    ["no error", null],
    ["another code", "E_GUI_TEMPLATE_WRITE_FAILED"],
  ])("B4: fails %s as OPEN_ERROR_NOT_CODED", (_label, errorCode) => {
    const failedOpen = { ...OPEN_PASSING.failedOpen, errorCode };
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, failedOpen }))).toEqual([
      LAUNCHER_GATE_CODES.OPEN_ERROR_NOT_CODED,
    ]);
  });

  it.each([
    ["a hidden start view", { startViewVisible: false }],
    ["a loaded document", { loaded: true }],
  ])("B4: fails a failed open leaving %s as OPEN_FAILURE_LEFT_START_VIEW", (_label, change) => {
    const failedOpen = { ...OPEN_PASSING.failedOpen, ...change };
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, failedOpen }))).toEqual([
      LAUNCHER_GATE_CODES.OPEN_FAILURE_LEFT_START_VIEW,
    ]);
  });

  it.each([
    ["DOCUMENT_NOT_LOADED", { loaded: false }],
    ["ACTIVE_FILE_MISMATCH", { activeFile: "/repo/examples/other.ri" }],
    ["NO_MESHES", { meshCount: 0 }],
    ["NOT_IN_RECENT", { recentFirstPath: "/repo/examples/other.ri" }],
    ["START_VIEW_STILL_SHOWN", { startViewVisible: true }],
  ] as const)("B2: fails as %s", (code, change) => {
    const opened = { ...OPEN_PASSING.opened, ...change };
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, opened }))).toEqual([LAUNCHER_GATE_CODES[code]]);
  });

  it("B2: a missing recent entry fails as NOT_IN_RECENT", () => {
    const opened = { ...OPEN_PASSING.opened, recentFirstPath: undefined };
    expect(codes(gradeOpenScenario({ ...OPEN_PASSING, opened }))).toEqual([
      LAUNCHER_GATE_CODES.NOT_IN_RECENT,
    ]);
  });
});

describe("gradeNewFileScenario", () => {
  it("passes a fully green observation", () => {
    expect(gradeNewFileScenario(NEW_PASSING)).toEqual([]);
  });

  it("B1: fails a launch without the start view as START_VIEW_NOT_SHOWN", () => {
    const startView = { loaded: false, startViewVisible: false };
    expect(codes(gradeNewFileScenario({ ...NEW_PASSING, startView }))).toEqual([
      LAUNCHER_GATE_CODES.START_VIEW_NOT_SHOWN,
    ]);
  });

  it("B3: fails a file missing from disk as FILE_NOT_CREATED", () => {
    const created = { ...NEW_PASSING.created, existsOnDisk: false, firstLine: null };
    expect(codes(gradeNewFileScenario({ ...NEW_PASSING, created }))).toEqual([
      LAUNCHER_GATE_CODES.FILE_NOT_CREATED,
    ]);
  });

  it.each([
    ["no module line", "structure def Flange {"],
    ["another module", "module other"],
  ])("B3: fails %s as MODULE_LINE_MISSING", (_label, firstLine) => {
    const created = { ...NEW_PASSING.created, firstLine };
    expect(codes(gradeNewFileScenario({ ...NEW_PASSING, created }))).toEqual([
      LAUNCHER_GATE_CODES.MODULE_LINE_MISSING,
    ]);
  });

  it.each([
    ["COMPILE_ERRORS", { compileErrorCount: 2 }],
    ["NO_MESHES", { meshCount: 0 }],
    ["DOCUMENT_NOT_LOADED", { loaded: false }],
    ["ACTIVE_FILE_MISMATCH", { activeFile: "/tmp/t/other.ri" }],
    ["START_VIEW_STILL_SHOWN", { startViewVisible: true }],
  ] as const)("B3: fails as %s", (code, change) => {
    expect(codes(gradeNewFileScenario({ ...NEW_PASSING, ...change }))).toEqual([LAUNCHER_GATE_CODES[code]]);
  });
});

describe("formatFailures", () => {
  it("renders one line per failure, led by its code, for every code", () => {
    const failures = Object.values(LAUNCHER_GATE_CODES).map((code) => ({ code }));

    const lines = formatFailures(failures).split("\n");

    expect(lines).toHaveLength(failures.length);
    lines.forEach((line, i) => {
      expect(line.startsWith(`[${failures[i].code}] `)).toBe(true);
      expect(line.length).toBeGreaterThan(`[${failures[i].code}] `.length);
    });
  });

  it("gives every code its own description rather than a JSON fallback", () => {
    for (const code of Object.values(LAUNCHER_GATE_CODES)) {
      expect(formatFailures([{ code }])).not.toContain(JSON.stringify({ code }));
    }
  });
});
