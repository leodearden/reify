import { rmSync, writeFileSync } from 'node:fs'

// Detects the worker->host RPC starvation signature in a vitest run (task 7630).
//
// Under heavy cross-worktree load the vitest host process can stall past
// birpc's hardcoded 60 s DEFAULT_TIMEOUT, and every worker->host call in flight
// rejects with `[vitest-worker]: Timeout calling "<method>"`. The suites
// carrying those calls fail before running a single test, so the run reports
// failed SUITES. The same stall can also land inside a test BODY, where the
// test's own timeout fires first: that test fails with vitest's "Test timed
// out", not with the RPC text (task 7833, esc-7094-6: Editor.test.tsx awaiting
// a `?raw` import). vitest 3.2.4 exposes no knob for birpc's bound
// (WorkerRpcOptions type-excludes `timeout`), and the merge lane is exempt from
// CPU admission by design, so the event cannot be prevented here — only
// recognised, reported, and recovered from.
//
// This module owns the recognition half. scripts/gui-vitest-run.sh owns the
// recovery half and reads its decision from the artifact, never from stdout.

/** One suite that failed before or during collection, as vitest reported it. */
export interface FailedSuiteRecord {
  readonly filepath: string
  readonly errorMessages: readonly string[]
}

/** One FAILED test: the module that holds it, and the test's own error messages. */
export interface FailedTestRecord {
  readonly filepath: string
  readonly errorMessages: readonly string[]
}

/** Everything the classifier needs about a finished run — no vitest types. */
export interface WorkerRpcFailureSummary {
  readonly failedSuites: readonly FailedSuiteRecord[]
  readonly failedTests: readonly FailedTestRecord[]
  /**
   * Errors vitest raised outside any suite. They also fail the run, and no
   * module is named by one — so ANY of them, alone or alongside failed suites,
   * yields a verdict naming no suite at all, which the runner reads as "re-run
   * the original invocation" rather than as "nothing to do". An unhandled error
   * that is not itself an RPC timeout still vetoes, exactly as an unexplained
   * suite failure does.
   */
  readonly unhandledErrorMessages?: readonly string[]
  /**
   * Suites that never reached a terminal state — vitest leaves them 'pending'
   * or 'queued'. Under starvation the forks pool can die partway, so a run can
   * report two RPC-timeout failures while twenty more suites simply never ran.
   * Retrying only the failures would green the gate on a run that never
   * executed them, so any unfinished suite vetoes.
   */
  readonly unfinishedSuites?: readonly string[]
  /**
   * vitest's TestRunEndReason: 'passed' | 'interrupted' | 'failed'. An
   * interrupted run was cut short, so nothing about it is a complete picture.
   */
  readonly runEndReason?: string
}

export interface WorkerRpcFlakeVerdict {
  readonly kind: 'worker_rpc_timeout'
  /**
   * What the retry should NARROW to — not an inventory of what failed. EMPTY
   * means "nothing to narrow to", which the runner reads as "re-run the
   * caller's original invocation". `methods` is where the event itself is
   * accounted for, and it covers run-level failures that no suite names.
   */
  readonly suites: readonly string[]
  readonly methods: readonly string[]
}

// Anchored at the start of the message because that is where vitest's
// createRuntimeRpc onTimeoutError writes it (dist/chunks/rpc.*.js:49); prose
// that merely quotes the phrase mid-sentence is not this failure.
const RPC_TIMEOUT_MESSAGE = /^\[vitest-worker\]: Timeout calling "([^"]+)"/

// vitest's own per-test timeout, from @vitest/runner makeTimeoutError
// (dist/chunk-hooks.js:2006). Anchored for the same reason; a HOOK timeout
// reads "Hook timed out" and is deliberately not this failure.
const TEST_TIMEOUT_MESSAGE = /^Test timed out in \d+ms\./

const timedOutMethod = (message: string): string | null =>
  RPC_TIMEOUT_MESSAGE.exec(message)?.[1] ?? null

const isRpcTimeout = (message: string): boolean => timedOutMethod(message) !== null

const rpcMethodsIn = (messages: readonly string[]): string[] =>
  messages.map(timedOutMethod).filter((method): method is string => method !== null)

const isStarvationShapedTest = (test: FailedTestRecord): boolean =>
  test.errorMessages.length > 0 &&
  test.errorMessages.every((message) => isRpcTimeout(message) || TEST_TIMEOUT_MESSAGE.test(message))

const starvedOutright = (suite: FailedSuiteRecord): boolean => suite.errorMessages.some(isRpcTimeout)

/**
 * Explained by its own RPC timeout or, carrying no error of its own, by the
 * failed test it holds — which isStarvationShapedTest has already vetted.
 */
const isExplainedSuite = (suite: FailedSuiteRecord, holdsFailedTest: ReadonlySet<string>): boolean =>
  starvedOutright(suite) || (suite.errorMessages.length === 0 && holdsFailedTest.has(suite.filepath))

/**
 * Returns a verdict when the run is UNAMBIGUOUSLY a host-starvation event, and
 * null otherwise. Each rule below makes that "unambiguously" true, and each is
 * load-bearing for the bounded retry this feeds:
 *
 *  - Something actually FAILED: a failed suite, a failed test, or an unhandled
 *    error. An all-green run is not a flake and has nothing to retry.
 *  - Every failed test is STARVATION-SHAPED: it carries at least one error, and
 *    EVERY one is an RPC timeout or vitest's own "Test timed out". An assertion
 *    failure therefore never classifies, even beside a timeout on the same test.
 *  - A failed test is CORROBORATED by at least one failed suite whose own
 *    errors carry an RPC timeout. A lone test timeout is exactly what a genuine
 *    hang looks like, and a run-level RPC timeout does not corroborate it.
 *  - Every failed suite is explained — by an RPC timeout among its errors or,
 *    carrying no error of its own, by the vetted failed test it holds — and
 *    every unhandled error is an RPC timeout. One failure arising any other way
 *    vetoes the whole run, so a real defect coinciding with a starvation event
 *    is never absorbed.
 *  - Every suite reached a terminal state and the run was not interrupted.
 *    The rules above reason only about failures that were REPORTED; this one
 *    closes the same hole for suites that never RAN, which a dying forks pool
 *    leaves behind. Without it a retry of the two failures could green a gate
 *    that silently skipped twenty more.
 *
 * The failed-test rules (task 7833) can absorb a genuine HANG only when it
 * coincides with independent suite-level starvation, and scope below always
 * re-runs the hung test's module — so a deterministic hang recurs on the one
 * bounded retry and escalates red.
 *
 * SCOPE is decided separately, and by ATTRIBUTABILITY rather than by counting.
 * A run-level failure has no module to attribute it to: the `snapshotSaved` RPC
 * is issued after a file's tests have already passed (task 7724, esc-7600-1),
 * and a timed-out `onUnhandledError` means a genuine unhandled error was lost
 * in transit from a module that may well have passed. So ANY unhandled error
 * makes the verdict name no suite at all — including when suites failed too,
 * where narrowing to them would re-run everything except the thing that has no
 * name. Otherwise the verdict names every module holding a failure, the failed
 * tests' modules included. Widening a retry can never mask a failure; narrowing
 * past the evidence can.
 */
export function classifyWorkerRpcFlake(
  summary: WorkerRpcFailureSummary,
): WorkerRpcFlakeVerdict | null {
  const { failedSuites, failedTests } = summary
  const unhandled = summary.unhandledErrorMessages ?? []
  const holdsFailedTest = new Set(failedTests.map((test) => test.filepath))

  if (failedSuites.length === 0 && failedTests.length === 0 && unhandled.length === 0) return null
  if (summary.runEndReason === 'interrupted') return null
  if ((summary.unfinishedSuites?.length ?? 0) !== 0) return null
  if (!failedTests.every(isStarvationShapedTest)) return null
  if (failedTests.length !== 0 && !failedSuites.some(starvedOutright)) return null
  if (!failedSuites.every((suite) => isExplainedSuite(suite, holdsFailedTest))) return null
  if (!unhandled.every(isRpcTimeout)) return null

  const methods = new Set([
    ...failedSuites.flatMap((suite) => rpcMethodsIn(suite.errorMessages)),
    ...failedTests.flatMap((test) => rpcMethodsIn(test.errorMessages)),
    ...rpcMethodsIn(unhandled),
  ])
  const failingModules = new Set([...failedSuites.map((s) => s.filepath), ...holdsFailedTest])

  return {
    kind: 'worker_rpc_timeout',
    suites: unhandled.length === 0 ? [...failingModules] : [],
    methods: [...methods].sort(),
  }
}

// ---------------------------------------------------------------------------
// The vitest-facing half: a thin adapter, then the two outputs. All decision
// logic stays in the classifier above, so the signature has one definition.
// ---------------------------------------------------------------------------

/**
 * The part of vitest's TestModule this reporter reads. Declared structurally
 * rather than imported so the adapter depends on four members instead of
 * vitest's whole reported-task surface.
 */
export interface ReportedModule {
  readonly moduleId: string
  state(): string
  errors(): ReadonlyArray<{ message?: string }>
  readonly children: { allTests(state?: string): Iterable<ReportedTest> }
}

/** The part of vitest's TestCase this reporter reads, declared the same way. */
export interface ReportedTest {
  result(): { readonly errors?: ReadonlyArray<{ message?: string }> }
}

/**
 * Where the reporter writes, relative to the gui root. Lives under
 * node_modules because that directory is already gitignored and npm ci
 * recreates it before every run, so the artifact can never be committed and
 * never outlives an install.
 *
 * SHARED CONSTANT: scripts/gui-vitest-run.sh reads this same path — it is the
 * single seam between the two halves. The infra suite pins that they agree.
 */
export const WORKER_RPC_FLAKE_ARTIFACT = 'node_modules/.reify-gui-rpc-flake.json'

/**
 * vitest's TestModuleState is `TestSuiteState | "queued"`, i.e.
 * 'skipped' | 'pending' | 'failed' | 'passed' | 'queued'
 * (node_modules/vitest/dist/chunks/reporters.d.*.d.ts:270-271). Only the first
 * three of those mean the module is DONE; 'pending' and 'queued' mean the run
 * never got to it.
 */
const TERMINAL_MODULE_STATES: ReadonlySet<string> = new Set(['passed', 'failed', 'skipped'])

/** The escalation history this marker belongs to, so a recurrence self-identifies. */
const LINEAGE = '3185,4856,7630'

/** Everything the reporter touches outside itself, injectable for testing. */
export interface FlakeReporterOutput {
  rootDir: string
  artifactPath: string
  emit: (line: string) => void
  writeArtifact: (path: string, contents: string) => void
  discardArtifact: (path: string) => void
  warn: (message: string) => void
}

const messageOf = (error: { message?: string } | undefined): string =>
  typeof error?.message === 'string' ? error.message : String(error)

const relativeTo = (rootDir: string, moduleId: string): string =>
  moduleId.startsWith(`${rootDir}/`) ? moduleId.slice(rootDir.length + 1) : moduleId

/** Every failed test in EVERY module, whatever state its module reports. */
const collectFailedTests = (
  rootDir: string,
  modules: readonly ReportedModule[],
): FailedTestRecord[] =>
  modules.flatMap((module) =>
    Array.from(module.children.allTests('failed'), (test) => ({
      filepath: relativeTo(rootDir, module.moduleId),
      errorMessages: (test.result().errors ?? []).map(messageOf),
    })),
  )

export default class WorkerRpcFlakeReporter {
  private readonly out: FlakeReporterOutput

  constructor(output: Partial<FlakeReporterOutput> = {}) {
    const rootDir = output.rootDir ?? process.cwd()
    this.out = {
      rootDir,
      artifactPath: output.artifactPath ?? `${rootDir}/${WORKER_RPC_FLAKE_ARTIFACT}`,
      emit: output.emit ?? ((line) => process.stdout.write(`${line}\n`)),
      // Synchronous by requirement, not by habit: vitest may exit as soon as
      // the last reporter hook returns, and a deferred write would be lost
      // exactly when the runner needs the artifact.
      writeArtifact: output.writeArtifact ?? writeFileSync,
      discardArtifact: output.discardArtifact ?? ((path) => rmSync(path, { force: true })),
      warn: output.warn ?? ((message) => process.stderr.write(`${message}\n`)),
    }
  }

  /**
   * A reporter that only diagnoses must never be the thing that fails a run.
   * Every artifact touch goes through here, so an absent node_modules, a
   * read-only mount or an ENOSPC lane degrades to "no artifact" — which the
   * runner already reads as "do not retry", the conservative outcome.
   */
  private tryIo(what: string, io: () => void): void {
    try {
      io()
    } catch (error) {
      this.out.warn(`WorkerRpcFlakeReporter: could not ${what}: ${String(error)}`)
    }
  }

  /** A stale artifact from an earlier run must never be read as this run's. */
  onTestRunStart(): void {
    this.tryIo('discard a stale flake artifact', () =>
      this.out.discardArtifact(this.out.artifactPath),
    )
  }

  onTestRunEnd(
    testModules: readonly ReportedModule[],
    unhandledErrors: ReadonlyArray<{ message?: string }> = [],
    reason?: string,
  ): void {
    const verdict = classifyWorkerRpcFlake({
      failedSuites: testModules
        .filter((module) => module.state() === 'failed')
        .map((module) => ({
          filepath: relativeTo(this.out.rootDir, module.moduleId),
          errorMessages: module.errors().map(messageOf),
        })),
      failedTests: collectFailedTests(this.out.rootDir, testModules),
      unhandledErrorMessages: unhandledErrors.map(messageOf),
      unfinishedSuites: testModules
        .filter((module) => !TERMINAL_MODULE_STATES.has(module.state()))
        .map((module) => relativeTo(this.out.rootDir, module.moduleId)),
      runEndReason: reason,
    })

    if (verdict === null) {
      this.tryIo('discard the flake artifact', () =>
        this.out.discardArtifact(this.out.artifactPath),
      )
      return
    }

    // `suites=0` alone would read as "retrying zero suites"; scope= says which
    // question the runner will re-ask. Derived from the verdict, so the two
    // keys cannot disagree.
    this.out.emit(
      `@@REIFY_GUI_FLAKE@@ kind=${verdict.kind}` +
        ` scope=${verdict.suites.length === 0 ? 'run' : 'suites'}` +
        ` suites=${verdict.suites.length} methods=${verdict.methods.join(',')}` +
        ` lineage=${LINEAGE}`,
    )
    this.tryIo('write the flake artifact', () =>
      this.out.writeArtifact(this.out.artifactPath, `${JSON.stringify(verdict, null, 2)}\n`),
    )
  }
}
