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
 * null otherwise. Two rules make that "unambiguously" true, and both are
 * load-bearing for the bounded retry this feeds:
 *
 *  - Zero failed tests. A genuine code defect produces failed tests, so a
 *    non-zero count can never be classified as starvation.
 *  - Every failed suite carries an RPC timeout. One suite failing for any
 *    other reason vetoes the whole run, so a real defect coinciding with a
 *    starvation event is never absorbed.
 */
export function classifyWorkerRpcFlake(
  summary: WorkerRpcFailureSummary,
): WorkerRpcFlakeVerdict | null {
  if (summary.failedTestCount !== 0) return null
  if (summary.failedSuites.length === 0) return null

  const methods = new Set<string>()
  for (const suite of summary.failedSuites) {
    const found = suite.errorMessages.map(timedOutMethod).filter((m): m is string => m !== null)
    if (found.length === 0) return null
    for (const method of found) methods.add(method)
  }

  return {
    kind: 'worker_rpc_timeout',
    suites: summary.failedSuites.map((s) => s.filepath),
    methods: [...methods].sort(),
  }
}
