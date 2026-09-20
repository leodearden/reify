import { rmSync, writeFileSync } from 'node:fs'

// Detects the worker->host RPC starvation signature in a vitest run (task 7630).
//
// Under heavy cross-worktree load the vitest host process can stall past
// birpc's hardcoded 60 s DEFAULT_TIMEOUT, and every worker->host call in flight
// rejects with `[vitest-worker]: Timeout calling "<method>"`. The suites
// carrying those calls fail before running a single test, so the run reports
// failed SUITES and zero failed TESTS. vitest 3.2.4 exposes no knob for that
// bound (WorkerRpcOptions type-excludes `timeout`), and the merge lane is
// exempt from CPU admission by design, so the event cannot be prevented here —
// only recognised, reported, and recovered from.
//
// This module owns the recognition half. scripts/gui-vitest-run.sh owns the
// recovery half and reads its decision from the artifact, never from stdout.

/** One suite that failed before or during collection, as vitest reported it. */
export interface FailedSuiteRecord {
  readonly filepath: string
  readonly errorMessages: readonly string[]
}

/** Everything the classifier needs about a finished run — no vitest types. */
export interface WorkerRpcFailureSummary {
  readonly failedSuites: readonly FailedSuiteRecord[]
  readonly failedTestCount: number
  /**
   * Errors vitest raised outside any suite. They also fail the run, and a run
   * that failed ONLY on these yields a verdict naming no suite at all — which
   * the runner reads as "re-run the original invocation", not as "nothing to
   * do". An unhandled error that is not itself an RPC timeout still vetoes,
   * exactly as an unexplained suite failure does.
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
  readonly suites: readonly string[]
  readonly methods: readonly string[]
}

// Anchored at the start of the message because that is where vitest's
// createRuntimeRpc onTimeoutError writes it (dist/chunks/rpc.*.js:49); prose
// that merely quotes the phrase mid-sentence is not this failure.
const RPC_TIMEOUT_MESSAGE = /^\[vitest-worker\]: Timeout calling "([^"]+)"/

const timedOutMethod = (message: string): string | null =>
  RPC_TIMEOUT_MESSAGE.exec(message)?.[1] ?? null

/**
 * Returns a verdict when the run is UNAMBIGUOUSLY a host-starvation event, and
 * null otherwise. Each rule below makes that "unambiguously" true, and each is
 * load-bearing for the bounded retry this feeds:
 *
 *  - Something actually FAILED: at least one failed suite, or one unhandled
 *    error. An all-green run is not a flake and has nothing to retry.
 *  - Zero failed tests. A genuine code defect produces failed tests, so a
 *    non-zero count can never be classified as starvation.
 *  - Every failed suite carries an RPC timeout, and so does every unhandled
 *    error. One failure of either kind arising any other way vetoes the whole
 *    run, so a real defect coinciding with a starvation event is never
 *    absorbed.
 *  - Every suite reached a terminal state and the run was not interrupted.
 *    The rules above reason only about failures that were REPORTED; this one
 *    closes the same hole for suites that never RAN, which a dying forks pool
 *    leaves behind. Without it a retry of the two failures could green a gate
 *    that silently skipped twenty more.
 *
 * The event can arrive with ZERO failed suites: the `snapshotSaved` RPC is
 * issued after a file's tests have already passed, so a timeout on it surfaces
 * at RUN level with no module to attribute it to (task 7724, esc-7600-1). The
 * verdict then names no suite — the complete statement that there is nothing
 * to narrow the retry to.
 */
export function classifyWorkerRpcFlake(
  summary: WorkerRpcFailureSummary,
): WorkerRpcFlakeVerdict | null {
  const unhandled = summary.unhandledErrorMessages ?? []

  if (summary.failedTestCount !== 0) return null
  if (summary.failedSuites.length === 0 && unhandled.length === 0) return null
  if (summary.runEndReason === 'interrupted') return null
  if ((summary.unfinishedSuites?.length ?? 0) !== 0) return null

  const methods = new Set<string>()
  for (const suite of summary.failedSuites) {
    const found = suite.errorMessages.map(timedOutMethod).filter((m): m is string => m !== null)
    if (found.length === 0) return null
    for (const method of found) methods.add(method)
  }
  for (const message of unhandled) {
    const method = timedOutMethod(message)
    if (method === null) return null
    methods.add(method)
  }

  return {
    kind: 'worker_rpc_timeout',
    suites: summary.failedSuites.map((s) => s.filepath),
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
  readonly children: { allTests(state?: string): Iterable<unknown> }
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

const countFailedTests = (modules: readonly ReportedModule[]): number => {
  let failed = 0
  for (const module of modules) for (const _ of module.children.allTests('failed')) failed++
  return failed
}

const messageOf = (error: { message?: string } | undefined): string =>
  typeof error?.message === 'string' ? error.message : String(error)

const relativeTo = (rootDir: string, moduleId: string): string =>
  moduleId.startsWith(`${rootDir}/`) ? moduleId.slice(rootDir.length + 1) : moduleId

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
      failedTestCount: countFailedTests(testModules),
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

    this.out.emit(
      `@@REIFY_GUI_FLAKE@@ kind=${verdict.kind} suites=${verdict.suites.length}` +
        ` methods=${verdict.methods.join(',')} lineage=${LINEAGE}`,
    )
    this.tryIo('write the flake artifact', () =>
      this.out.writeArtifact(this.out.artifactPath, `${JSON.stringify(verdict, null, 2)}\n`),
    )
  }
}
