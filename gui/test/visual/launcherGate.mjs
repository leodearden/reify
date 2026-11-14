/**
 * Every decision the live launcher smoke (`./smoke_launcher_e2e.mjs`, task
 * 9730, alpha-spine β B1–B5) makes, as pure data. The `open` scenario launches
 * with no file and checks the Start view (B1), Debug mode on the env-chosen
 * port, idempotent on re-selection (B5), a coded failed open that keeps the
 * Start view (B4) and a successful open (B2); the `new` scenario checks the
 * Start view (B1) and a New… design that is written, declared, compiles and
 * renders (B3). Each check returns failure records whose `code` is one of
 * {@link LAUNCHER_GATE_CODES}; {@link formatFailures} is the single place one
 * becomes English.
 *
 * PLAIN ESM, free of builtin-module imports: both the vitest suite
 * (`./launcherGate.test.ts`) and the live driver (run with bare `node`) load it.
 */

export const LAUNCHER_GATE_CODES = Object.freeze({
  /** A no-file launch did not show the Start view with no document loaded. */
  START_VIEW_NOT_SHOWN: "start-view-not-shown",
  /** Debug mode did not report the debug server on the env-chosen port. */
  DEBUG_PORT_MISMATCH: "debug-port-mismatch",
  /** Selecting Debug again moved the port, left Debug mode or killed the server. */
  DEBUG_NOT_IDEMPOTENT: "debug-not-idempotent",
  /** Opening a missing path did not surface E_GUI_OPEN_FAILED. */
  OPEN_ERROR_NOT_CODED: "open-error-not-coded",
  /** A failed open loaded a document or took the Start view down. */
  OPEN_FAILURE_LEFT_START_VIEW: "open-failure-left-start-view",
  /** No document was loaded after the open or create. */
  DOCUMENT_NOT_LOADED: "document-not-loaded",
  /** The editor's active file is not the one opened or created. */
  ACTIVE_FILE_MISMATCH: "active-file-mismatch",
  /** The loaded design rendered no meshes. */
  NO_MESHES: "no-meshes",
  /** The opened document is not first in the Recent list. */
  NOT_IN_RECENT: "not-in-recent",
  /** The Start view was still shown with a document loaded. */
  START_VIEW_STILL_SHOWN: "start-view-still-shown",
  /** New… left no file on disk. */
  FILE_NOT_CREATED: "file-not-created",
  /** The created file does not open with `module <stem>`. */
  MODULE_LINE_MISSING: "module-line-missing",
  /** The created design has compile errors. */
  COMPILE_ERRORS: "compile-errors",
});

const OPEN_FAILED = "E_GUI_OPEN_FAILED";

const basename = (path) => (typeof path === "string" ? path.split("/").pop() : undefined);

const isDebugOn = (mode, port) => mode?.selected === "debug" && mode?.debugPort === port;

/** B1: a no-file launch shows the Start view with nothing loaded. */
function gradeStartView({ loaded, startViewVisible }) {
  return loaded === false && startViewVisible === true
    ? []
    : [{ code: LAUNCHER_GATE_CODES.START_VIEW_NOT_SHOWN, loaded, startViewVisible }];
}

/** B5: Debug mode on the env port at launch and on selection; a re-selection changes nothing. */
function gradeDebugMode({ debugPortEnv, atLaunch, first, second, healthAfter }) {
  const failures = [];
  if (!isDebugOn(atLaunch, debugPortEnv) || !isDebugOn(first, debugPortEnv)) {
    failures.push({ code: LAUNCHER_GATE_CODES.DEBUG_PORT_MISMATCH, debugPortEnv, atLaunch, first });
  }
  if (!isDebugOn(second, first?.debugPort) || healthAfter !== true) {
    failures.push({ code: LAUNCHER_GATE_CODES.DEBUG_NOT_IDEMPOTENT, first, second, healthAfter });
  }
  return failures;
}

/** B4: a failed open is coded and keeps the Start view up with nothing loaded. */
function gradeFailedOpen({ loaded, errorCode, startViewVisible }) {
  const failures = [];
  if (errorCode !== OPEN_FAILED) {
    failures.push({ code: LAUNCHER_GATE_CODES.OPEN_ERROR_NOT_CODED, errorCode });
  }
  if (loaded !== false || startViewVisible !== true) {
    failures.push({ code: LAUNCHER_GATE_CODES.OPEN_FAILURE_LEFT_START_VIEW, loaded, startViewVisible });
  }
  return failures;
}

/** A document that is loaded, active, rendered and has taken the Start view down. */
function gradeLoadedDocument({ loaded, activeFile, fileName, meshCount, startViewVisible }) {
  const failures = [];
  if (loaded !== true) failures.push({ code: LAUNCHER_GATE_CODES.DOCUMENT_NOT_LOADED, loaded });
  if (basename(activeFile) !== fileName) {
    failures.push({ code: LAUNCHER_GATE_CODES.ACTIVE_FILE_MISMATCH, activeFile, fileName });
  }
  if (!(typeof meshCount === "number" && meshCount > 0)) {
    failures.push({ code: LAUNCHER_GATE_CODES.NO_MESHES, meshCount });
  }
  if (startViewVisible !== false) {
    failures.push({ code: LAUNCHER_GATE_CODES.START_VIEW_STILL_SHOWN, startViewVisible });
  }
  return failures;
}

/** B2: the fixture opens, renders and becomes the first Recent entry. */
function gradeOpened({ loaded, activeFile, fixtureName, meshCount, recentFirstPath, startViewVisible }) {
  const failures = gradeLoadedDocument({ loaded, activeFile, fileName: fixtureName, meshCount, startViewVisible });
  if (basename(recentFirstPath) !== fixtureName) {
    failures.push({ code: LAUNCHER_GATE_CODES.NOT_IN_RECENT, recentFirstPath, fixtureName });
  }
  return failures;
}

/**
 * Grade the `open` scenario: B1, B5, B4 and B2 in the order the driver runs them.
 *
 * @param {{
 *   startView: {loaded: unknown, startViewVisible: unknown},
 *   debugMode: {
 *     debugPortEnv: number,
 *     atLaunch: {selected: unknown, debugPort: unknown},
 *     first: {selected: unknown, debugPort: unknown},
 *     second: {selected: unknown, debugPort: unknown},
 *     healthAfter: unknown,
 *   },
 *   failedOpen: {loaded: unknown, errorCode: unknown, startViewVisible: unknown},
 *   opened: {
 *     loaded: unknown, activeFile: unknown, fixtureName: string, meshCount: unknown,
 *     recentFirstPath: unknown, startViewVisible: unknown,
 *   },
 * }} obs
 * @returns {Array<{code: string} & Record<string, unknown>>}
 */
export function gradeOpenScenario({ startView, debugMode, failedOpen, opened }) {
  return [
    ...gradeStartView(startView),
    ...gradeDebugMode(debugMode),
    ...gradeFailedOpen(failedOpen),
    ...gradeOpened(opened),
  ];
}

/**
 * Grade the `new` scenario: B1, then B3 — the New… design on disk and loaded.
 *
 * @param {{
 *   startView: {loaded: unknown, startViewVisible: unknown},
 *   created: {existsOnDisk: unknown, firstLine: unknown, stem: string},
 *   loaded: unknown, activeFile: unknown, compileErrorCount: unknown,
 *   meshCount: unknown, startViewVisible: unknown,
 * }} obs
 * @returns {Array<{code: string} & Record<string, unknown>>}
 */
export function gradeNewFileScenario({ startView, created, loaded, activeFile, compileErrorCount, meshCount, startViewVisible }) {
  const failures = gradeStartView(startView);
  if (created.existsOnDisk !== true) {
    failures.push({ code: LAUNCHER_GATE_CODES.FILE_NOT_CREATED });
  } else if (created.firstLine !== `module ${created.stem}`) {
    failures.push({ code: LAUNCHER_GATE_CODES.MODULE_LINE_MISSING, firstLine: created.firstLine, stem: created.stem });
  }
  if (compileErrorCount !== 0) {
    failures.push({ code: LAUNCHER_GATE_CODES.COMPILE_ERRORS, compileErrorCount });
  }
  failures.push(
    ...gradeLoadedDocument({ loaded, activeFile, fileName: `${created.stem}.ri`, meshCount, startViewVisible }),
  );
  return failures;
}

const json = (value) => JSON.stringify(value);

const DESCRIBE = {
  [LAUNCHER_GATE_CODES.START_VIEW_NOT_SHOWN]: (f) =>
    `a no-file launch read document.loaded=${json(f.loaded)} and start view visible=${json(f.startViewVisible)} (want false, true)`,
  [LAUNCHER_GATE_CODES.DEBUG_PORT_MISMATCH]: (f) =>
    `debug mode read ${json(f.atLaunch)} at launch and ${json(f.first)} on selecting Debug (want selected "debug" on port ${f.debugPortEnv})`,
  [LAUNCHER_GATE_CODES.DEBUG_NOT_IDEMPOTENT]: (f) =>
    `selecting Debug again read ${json(f.second)} after ${json(f.first)}, health answered=${json(f.healthAfter)} (want the same mode and port, health true)`,
  [LAUNCHER_GATE_CODES.OPEN_ERROR_NOT_CODED]: (f) =>
    `opening a missing path set document error code ${json(f.errorCode)} (want "${OPEN_FAILED}")`,
  [LAUNCHER_GATE_CODES.OPEN_FAILURE_LEFT_START_VIEW]: (f) =>
    `after a failed open document.loaded=${json(f.loaded)} and start view visible=${json(f.startViewVisible)} (want false, true)`,
  [LAUNCHER_GATE_CODES.DOCUMENT_NOT_LOADED]: (f) => `document.loaded read ${json(f.loaded)} (want true)`,
  [LAUNCHER_GATE_CODES.ACTIVE_FILE_MISMATCH]: (f) =>
    `the active file is ${json(f.activeFile)} (want a file named ${f.fileName})`,
  [LAUNCHER_GATE_CODES.NO_MESHES]: (f) => `engine.meshCount read ${json(f.meshCount)} (want > 0)`,
  [LAUNCHER_GATE_CODES.NOT_IN_RECENT]: (f) =>
    `the first Recent entry is ${json(f.recentFirstPath)} (want a file named ${f.fixtureName})`,
  [LAUNCHER_GATE_CODES.START_VIEW_STILL_SHOWN]: (f) =>
    `the start view was still visible (${json(f.startViewVisible)}) with a document loaded`,
  [LAUNCHER_GATE_CODES.FILE_NOT_CREATED]: () => "New… left no file on disk",
  [LAUNCHER_GATE_CODES.MODULE_LINE_MISSING]: (f) =>
    `the created file starts ${json(f.firstLine)} (want "module ${f.stem}")`,
  [LAUNCHER_GATE_CODES.COMPILE_ERRORS]: (f) =>
    `the created design has ${json(f.compileErrorCount)} compile errors (want 0)`,
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
