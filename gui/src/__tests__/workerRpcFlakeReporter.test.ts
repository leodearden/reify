import { describe, it, expect } from 'vitest'
import WorkerRpcFlakeReporter, {
  classifyWorkerRpcFlake,
  WORKER_RPC_FLAKE_ARTIFACT,
  type ReportedModule,
  type WorkerRpcFailureSummary,
} from '../../vitest-worker-rpc-flake-reporter'

// The exact shape vitest 3.2.4 emits from createRuntimeRpc's onTimeoutError
// (node_modules/vitest/dist/chunks/rpc.*.js:49): the prefix is fixed, and the
// ` with "<args>"` tail is appended only for fetch/transform/resolveId (plus a
// message-only tail for onUnhandledError). The classifier must key off the
// prefix, never the tail.
const rpcTimeout = (method: string, tail?: string) =>
  `[vitest-worker]: Timeout calling "${method}"` + (tail === undefined ? '' : ` with "${tail}"`)

const summary = (
  failedSuites: WorkerRpcFailureSummary['failedSuites'],
  failedTestCount = 0,
  rest: Partial<WorkerRpcFailureSummary> = {},
): WorkerRpcFailureSummary => ({ failedSuites, failedTestCount, ...rest })

/** The recorded 7431 signature, reused wherever a POSITIVE input is needed. */
const starvedSuites = (): WorkerRpcFailureSummary['failedSuites'] => [
  { filepath: 'src/__tests__/engineStore.test.ts', errorMessages: [rpcTimeout('fetch', '[\\"x\\"]')] },
]

describe('classifyWorkerRpcFlake', () => {
  it('classifies the recorded 7431 signature: RPC timeouts with zero failed tests', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([
        {
          // The failed SUITE is a test file; gui/vitest.setup.ts is the module
          // it was fetching when the host stalled — the fetch ARGUMENT, not the
          // suite. isolate:true refetches the one setupFile per test file, which
          // is why that argument recurs across every recorded occurrence.
          filepath: 'src/__tests__/engineStore.test.ts',
          errorMessages: [
            rpcTimeout('fetch', '[\\"/home/leo/src/reify/gui/vitest.setup.ts\\",\\"web\\"]'),
          ],
        },
        {
          filepath: 'src/__tests__/meshManager.attributeResize.test.ts',
          errorMessages: [
            rpcTimeout(
              'fetch',
              '[\\"/home/leo/src/reify/gui/src/__tests__/meshManager.attributeResize.test.ts\\",\\"web\\"]',
            ),
          ],
        },
      ]),
    )

    expect(verdict).not.toBeNull()
    expect(verdict!.kind).toBe('worker_rpc_timeout')
    expect(verdict!.suites).toEqual([
      'src/__tests__/engineStore.test.ts',
      'src/__tests__/meshManager.attributeResize.test.ts',
    ])
    // Both suites timed out on `fetch`; the method set is de-duplicated.
    expect(verdict!.methods).toEqual(['fetch'])
  })

  // THE LOAD-BEARING VETO. A genuine code defect always produces failed TESTS.
  // Without this rule the retry in scripts/gui-vitest-run.sh could re-run — and
  // so mask — a real failure that happened to coincide with a starvation event.
  it('returns null when any test failed, even with a perfect RPC-timeout signature', () => {
    const verdict = classifyWorkerRpcFlake(
      summary(
        [
          {
            filepath: 'src/__tests__/engineStore.test.ts',
            errorMessages: [rpcTimeout('fetch', '[\\"…/engineStore.test.ts\\",\\"web\\"]')],
          },
        ],
        1,
      ),
    )

    expect(verdict).toBeNull()
  })

  it('returns null for an ordinary suite failure (module resolution / syntax)', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([
        {
          filepath: 'src/__tests__/surfaceManager.test.ts',
          errorMessages: [
            "Failed to resolve import \"../utils/nope\" from \"src/__tests__/surfaceManager.test.ts\". Does the file exist?",
          ],
        },
      ]),
    )

    expect(verdict).toBeNull()
  })

  // All-or-nothing: one unexplained suite failure vetoes the whole
  // classification, so a real defect riding alongside a starvation event is
  // never absorbed by the retry.
  it('returns null when only SOME failed suites carry an RPC timeout', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([
        {
          filepath: 'src/__tests__/engineStore.test.ts',
          errorMessages: [rpcTimeout('fetch', '[\\"…/vitest.setup.ts\\",\\"web\\"]')],
        },
        {
          filepath: 'src/__tests__/surfaceManager.test.ts',
          errorMessages: ['SyntaxError: Unexpected token }'],
        },
      ]),
    )

    expect(verdict).toBeNull()
  })

  // THE SECOND LOAD-BEARING VETO, and the one the rules above cannot see: they
  // reason only about suites that FAILED. A starved forks pool can die partway,
  // leaving suites 'queued' that never ran at all — retrying the two that failed
  // would green a gate that silently skipped the rest.
  it('returns null when any suite never reached a terminal state', () => {
    expect(
      classifyWorkerRpcFlake(
        summary(starvedSuites(), 0, { unfinishedSuites: ['src/__tests__/never-ran.test.ts'] }),
      ),
    ).toBeNull()
  })

  it('classifies normally when the unfinished-suite list is empty', () => {
    expect(classifyWorkerRpcFlake(summary(starvedSuites(), 0, { unfinishedSuites: [] }))).not.toBeNull()
  })

  it('returns null when the run was interrupted, however clean the signature', () => {
    expect(
      classifyWorkerRpcFlake(summary(starvedSuites(), 0, { runEndReason: 'interrupted' })),
    ).toBeNull()
  })

  it.each(['passed', 'failed'])('still classifies when the run ended as "%s"', (reason) => {
    expect(classifyWorkerRpcFlake(summary(starvedSuites(), 0, { runEndReason: reason }))).not.toBeNull()
  })

  it('returns null when no suites failed at all', () => {
    expect(classifyWorkerRpcFlake(summary([]))).toBeNull()
  })

  // THE RUN-SCOPE SHAPE (task 7724; recorded as esc-7600-1). `snapshotSaved` is
  // issued after a file's tests have already passed, so its timeout surfaces at
  // RUN level with no module to attribute it to: the run reports zero failed
  // suites, zero failed tests, and unhandled errors that are themselves RPC
  // timeouts. It must be told apart from the all-green run below — conflating
  // the two is exactly what left this event unclassified.
  it('classifies a run whose ONLY failures are run-level RPC timeouts', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([], 0, {
        unhandledErrorMessages: [rpcTimeout('snapshotSaved'), rpcTimeout('snapshotSaved')],
        runEndReason: 'failed',
      }),
    )

    expect(verdict).not.toBeNull()
    expect(verdict!.kind).toBe('worker_rpc_timeout')
    // No suite to narrow to; the runner reads this as "re-run what was asked".
    expect(verdict!.suites).toEqual([])
    expect(verdict!.methods).toEqual(['snapshotSaved'])
  })

  it('returns null for an all-green run: nothing failed, so there is nothing to retry', () => {
    expect(classifyWorkerRpcFlake(summary([], 0, { unhandledErrorMessages: [] }))).toBeNull()
  })

  // The all-or-nothing veto still holds when a run-level error is the only
  // evidence there is.
  it('returns null when the only run-level error is not an RPC timeout', () => {
    expect(
      classifyWorkerRpcFlake(
        summary([], 0, {
          unhandledErrorMessages: ['Error: unhandled rejection in a passing suite'],
        }),
      ),
    ).toBeNull()
  })

  it('returns null when the run-level errors are a MIX of RPC and non-RPC', () => {
    expect(
      classifyWorkerRpcFlake(
        summary([], 0, {
          unhandledErrorMessages: [rpcTimeout('snapshotSaved'), 'SyntaxError: boom'],
        }),
      ),
    ).toBeNull()
  })

  // A dying forks pool is not absorbed just because no suite was marked failed.
  it('returns null for the run-scope shape when a suite never reached a terminal state', () => {
    expect(
      classifyWorkerRpcFlake(
        summary([], 0, {
          unhandledErrorMessages: [rpcTimeout('snapshotSaved')],
          unfinishedSuites: ['src/__tests__/never-ran.test.ts'],
        }),
      ),
    ).toBeNull()
  })

  it('returns null for the run-scope shape when the run was interrupted', () => {
    expect(
      classifyWorkerRpcFlake(
        summary([], 0, {
          unhandledErrorMessages: [rpcTimeout('snapshotSaved')],
          runEndReason: 'interrupted',
        }),
      ),
    ).toBeNull()
  })

  it('returns null for a failed suite carrying no error messages', () => {
    expect(
      classifyWorkerRpcFlake(summary([{ filepath: 'src/__tests__/x.test.ts', errorMessages: [] }])),
    ).toBeNull()
  })

  // All four were observed timing out alongside `fetch` in the recorded runs.
  // They share the one 60 s birpc bound, so they are the same kind of event.
  it.each(['onQueued', 'snapshotSaved', 'onUnhandledError', 'resolveSnapshotPath'])(
    'recognises a timeout on "%s" as the same kind',
    (method) => {
      const verdict = classifyWorkerRpcFlake(
        summary([{ filepath: 'src/__tests__/a.test.ts', errorMessages: [rpcTimeout(method)] }]),
      )

      expect(verdict).not.toBeNull()
      expect(verdict!.kind).toBe('worker_rpc_timeout')
      expect(verdict!.methods).toEqual([method])
    },
  )

  it('reports the sorted, de-duplicated union of methods across suites', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([
        { filepath: 'a.test.ts', errorMessages: [rpcTimeout('snapshotSaved')] },
        { filepath: 'b.test.ts', errorMessages: [rpcTimeout('fetch', '[\\"b\\"]')] },
        { filepath: 'c.test.ts', errorMessages: [rpcTimeout('snapshotSaved')] },
        { filepath: 'd.test.ts', errorMessages: [rpcTimeout('onQueued')] },
      ]),
    )

    expect(verdict!.methods).toEqual(['fetch', 'onQueued', 'snapshotSaved'])
  })

  it('classifies a suite whose RPC timeout is one error among several', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([
        {
          filepath: 'src/__tests__/a.test.ts',
          errorMessages: ['Error: teardown warning', rpcTimeout('onUnhandledError', 'boom')],
        },
      ]),
    )

    expect(verdict!.methods).toEqual(['onUnhandledError'])
  })

  it('does not match a message that merely mentions the phrase in prose', () => {
    const verdict = classifyWorkerRpcFlake(
      summary([
        {
          filepath: 'src/__tests__/a.test.ts',
          errorMessages: ['expected log to contain Timeout calling "fetch" but it did not'],
        },
      ]),
    )

    expect(verdict).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// The reporter's two OUTPUTS: one column-0 marker line, and one JSON artifact.
// Driven through the public reporter entry point with synthetic module records
// and injected IO, so these tests touch no real filesystem and assume no cwd.
// ---------------------------------------------------------------------------

/**
 * A synthetic stand-in for vitest's TestModule, satisfying ReportedModule.
 * `state` accepts any TestModuleState ('skipped' | 'pending' | 'failed' |
 * 'passed' | 'queued'); `failed` is the shorthand for the common two.
 */
const testModule = (
  moduleId: string,
  opts: { failed?: boolean; state?: string; errors?: string[]; failedTests?: number } = {},
): ReportedModule => ({
  moduleId,
  state: () => opts.state ?? (opts.failed ? 'failed' : 'passed'),
  errors: () => (opts.errors ?? []).map((message) => ({ message })),
  children: {
    allTests: (state?: string) =>
      state === 'failed' ? new Array(opts.failedTests ?? 0).fill(null) : [],
  },
})

const ROOT = '/lane/gui'

interface Recorder {
  readonly reporter: WorkerRpcFlakeReporter
  readonly lines: string[]
  readonly writes: { path: string; contents: string }[]
  readonly discards: string[]
  readonly warnings: string[]
}

/**
 * `artifactPath: null` constructs the reporter WITHOUT one, exercising the
 * constructor's default. `overrides` lets a test make an IO channel throw
 * without touching a disk.
 */
const recordingReporter = (
  artifactPath: string | null = '/tmp/flake.json',
  overrides: Partial<{
    rootDir: string
    writeArtifact: (path: string, contents: string) => void
    discardArtifact: (path: string) => void
  }> = {},
): Recorder => {
  const lines: string[] = []
  const writes: { path: string; contents: string }[] = []
  const discards: string[] = []
  const warnings: string[] = []
  const reporter = new WorkerRpcFlakeReporter({
    rootDir: ROOT,
    ...(artifactPath === null ? {} : { artifactPath }),
    emit: (line) => lines.push(line),
    writeArtifact: (path, contents) => writes.push({ path, contents }),
    discardArtifact: (path) => discards.push(path),
    warn: (message) => warnings.push(message),
    ...overrides,
  })
  return { reporter, lines, writes, discards, warnings }
}

const TIMEOUT_FETCH = '[vitest-worker]: Timeout calling "fetch" with "[\\"x\\",\\"web\\"]"'
const TIMEOUT_SNAPSHOT = '[vitest-worker]: Timeout calling "snapshotSaved"'

const starvedRun = (): ReportedModule[] => [
  testModule(`${ROOT}/src/__tests__/engineStore.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] }),
  testModule(`${ROOT}/src/__tests__/meshManager.attributeResize.test.ts`, {
    failed: true,
    errors: [TIMEOUT_SNAPSHOT],
  }),
  testModule(`${ROOT}/src/__tests__/diff.test.ts`),
]

/**
 * The recorded 7724 run-scope signature: every module PASSED, and the only
 * failure is a run-level `snapshotSaved` timeout with no module to attribute
 * it to. Paired with the run-level error below, since neither half is the
 * signature on its own.
 */
const passedRun = (): ReportedModule[] => [
  testModule(`${ROOT}/src/__tests__/a.test.ts`),
  testModule(`${ROOT}/src/__tests__/b.test.ts`),
]
const RUN_LEVEL_TIMEOUT = [{ message: TIMEOUT_SNAPSHOT }]

describe('WorkerRpcFlakeReporter — marker line', () => {
  it('emits exactly one column-0 @@REIFY_GUI_FLAKE@@ line on the positive signature', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines).toHaveLength(1)
    expect(lines[0].startsWith('@@REIFY_GUI_FLAKE@@ ')).toBe(true)
  })

  it('carries kind, scope, suite count and the method csv in the established key=value grammar', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines[0]).toContain('kind=worker_rpc_timeout')
    expect(lines[0]).toContain('scope=suites')
    expect(lines[0]).toContain('suites=2')
    expect(lines[0]).toContain('methods=fetch,snapshotSaved')
  })

  // `suites=0` alone reads to an operator as "retrying zero suites", which is
  // the re-diagnosis cost esc-7600-1 paid. These two pin that the marker says
  // WHAT WILL BE RE-RUN, and that it varies with the verdict rather than being
  // a constant.
  it('says scope=run when the verdict names no suite to narrow to', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(passedRun(), RUN_LEVEL_TIMEOUT, 'failed')

    expect(lines[0]).toContain('scope=run')
    expect(lines[0]).toContain('suites=0')
  })

  it('says scope=suites when the verdict names the suites that failed', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines[0]).toContain('scope=suites')
    expect(lines[0]).toContain('suites=2')
  })

  // Self-identifying: a recurrence must name its own lineage so the next
  // responder reads the history off the line instead of re-diagnosing it.
  it('carries the task lineage so a recurrence needs no re-diagnosis', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines[0]).toContain('lineage=3185,4856,7630')
  })

  it('emits no newline of its own inside the marker (one grep-able line)', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines[0]).not.toContain('\n')
  })
})

describe('WorkerRpcFlakeReporter — JSON artifact', () => {
  it('writes the artifact to the caller-supplied path', () => {
    const { reporter, writes } = recordingReporter('/tmp/somewhere/flake.json')
    reporter.onTestRunEnd(starvedRun(), [])

    expect(writes).toHaveLength(1)
    expect(writes[0].path).toBe('/tmp/somewhere/flake.json')
  })

  it('names the affected suites as ROOT-RELATIVE paths the runner can pass to vitest', () => {
    const { reporter, writes } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(JSON.parse(writes[0].contents).suites).toEqual([
      'src/__tests__/engineStore.test.ts',
      'src/__tests__/meshManager.attributeResize.test.ts',
    ])
  })

  it('records the kind and the method set alongside the suites', () => {
    const { reporter, writes } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    const artifact = JSON.parse(writes[0].contents)
    expect(artifact.kind).toBe('worker_rpc_timeout')
    expect(artifact.methods).toEqual(['fetch', 'snapshotSaved'])
  })

  // The run-scope shape driven through the REPORTER, not just the classifier:
  // every module passed and the only failure is one run-level RPC timeout. Two
  // outputs, exactly one of each, and a suite list that is EMPTY rather than
  // absent — that emptiness is what tells the runner it has nothing to narrow to.
  it('writes one artifact naming NO suite when the only failure is a run-level RPC timeout', () => {
    const { reporter, lines, writes } = recordingReporter()
    reporter.onTestRunEnd(passedRun(), RUN_LEVEL_TIMEOUT, 'failed')

    expect(lines).toHaveLength(1)
    expect(writes).toHaveLength(1)
    const artifact = JSON.parse(writes[0].contents)
    expect(artifact.suites).toEqual([])
    expect(artifact.methods).toEqual(['snapshotSaved'])
  })

  // The constructor's default is the real seam with scripts/gui-vitest-run.sh's
  // "$GUI_DIR/node_modules/.reify-gui-rpc-flake.json", so drive it rather than
  // asserting the exported constant equals itself.
  it('defaults the artifact path to rootDir + the shared constant when none is injected', () => {
    const { reporter, writes } = recordingReporter(null)
    reporter.onTestRunEnd(starvedRun(), [])

    expect(writes).toHaveLength(1)
    expect(writes[0].path).toBe(`${ROOT}/${WORKER_RPC_FLAKE_ARTIFACT}`)
  })

  it('discards that same defaulted path at run start', () => {
    const { reporter, discards } = recordingReporter(null)
    reporter.onTestRunStart()

    expect(discards).toEqual([`${ROOT}/node_modules/.reify-gui-rpc-flake.json`])
  })
})

describe('WorkerRpcFlakeReporter — the negatives write nothing', () => {
  // The artifact's ABSENCE is what vetoes the retry in scripts/gui-vitest-run.sh,
  // so every one of these is load-bearing, not merely tidy.
  const cases: Array<[string, () => ReportedModule[], unknown[]]> = [
    [
      'a failed test alongside a perfect RPC signature',
      () => [
        testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH], failedTests: 1 }),
      ],
      [],
    ],
    [
      'an ordinary suite failure',
      () => [testModule(`${ROOT}/a.test.ts`, { failed: true, errors: ['SyntaxError: boom'] })],
      [],
    ],
    [
      'only some failed suites carrying an RPC timeout',
      () => [
        testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] }),
        testModule(`${ROOT}/b.test.ts`, { failed: true, errors: ['SyntaxError: boom'] }),
      ],
      [],
    ],
    ['an all-green run', () => [testModule(`${ROOT}/a.test.ts`)], []],
    [
      'a suite left queued when the starved forks pool died partway',
      () => [
        testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] }),
        testModule(`${ROOT}/never-ran.test.ts`, { state: 'queued' }),
      ],
      [],
    ],
    [
      'a suite still pending when the run ended',
      () => [
        testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] }),
        testModule(`${ROOT}/mid-flight.test.ts`, { state: 'pending' }),
      ],
      [],
    ],
    [
      'an unrelated unhandled error riding alongside the starvation event',
      () => [testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] })],
      [{ message: 'Error: unhandled rejection in a passing suite' }],
    ],
  ]

  it.each(cases)('emits no marker and writes no artifact for %s', (_name, modules, unhandled) => {
    const { reporter, lines, writes } = recordingReporter()
    reporter.onTestRunEnd(modules(), unhandled as never[])

    expect(lines).toEqual([])
    expect(writes).toEqual([])
  })

  // vitest passes the run's TestRunEndReason as onTestRunEnd's third argument.
  // 'interrupted' means the run was cut short, so its module list is not a
  // complete picture and nothing in it can be called a mere flake.
  it('emits no marker and writes no artifact when the run was interrupted', () => {
    const { reporter, lines, writes } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [], 'interrupted')

    expect(lines).toEqual([])
    expect(writes).toEqual([])
  })

  it('still classifies on the reasons that mean the run ran to completion', () => {
    for (const reason of ['passed', 'failed']) {
      const { reporter, writes } = recordingReporter()
      reporter.onTestRunEnd(starvedRun(), [], reason)

      expect(writes).toHaveLength(1)
    }
  })

  // A 'skipped' module IS terminal — vitest ran the file's collection and
  // decided not to run it. It must not be mistaken for a never-ran suite.
  it('treats a skipped suite as terminal, not as one that never ran', () => {
    const { reporter, writes } = recordingReporter()
    reporter.onTestRunEnd(
      [
        testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] }),
        testModule(`${ROOT}/b.test.ts`, { state: 'skipped' }),
      ],
      [],
      'failed',
    )

    expect(writes).toHaveLength(1)
  })

  // An unhandled error that IS the same starvation event must not veto: the
  // onUnhandledError RPC shares the one 60 s bound and was observed timing out.
  it('still classifies when the unhandled error is itself an RPC timeout', () => {
    const { reporter, writes } = recordingReporter()
    reporter.onTestRunEnd(
      [testModule(`${ROOT}/a.test.ts`, { failed: true, errors: [TIMEOUT_FETCH] })],
      [{ message: '[vitest-worker]: Timeout calling "onUnhandledError" with "boom"' }],
    )

    expect(writes).toHaveLength(1)
    expect(JSON.parse(writes[0].contents).methods).toEqual(['fetch', 'onUnhandledError'])
  })
})

describe('WorkerRpcFlakeReporter — stale artifacts never leak into a later run', () => {
  it('discards any stale artifact when a run starts', () => {
    const { reporter, discards } = recordingReporter('/tmp/flake.json')
    reporter.onTestRunStart()

    expect(discards).toEqual(['/tmp/flake.json'])
  })

  it('discards the artifact when a finished run does NOT match the signature', () => {
    const { reporter, discards, writes } = recordingReporter('/tmp/flake.json')
    reporter.onTestRunEnd([testModule(`${ROOT}/a.test.ts`)], [])

    expect(writes).toEqual([])
    expect(discards).toEqual(['/tmp/flake.json'])
  })

  it('does not discard the artifact it just wrote', () => {
    const { reporter, discards, writes } = recordingReporter('/tmp/flake.json')
    reporter.onTestRunEnd(starvedRun(), [])

    expect(writes).toHaveLength(1)
    expect(discards).toEqual([])
  })
})

// A reporter that exists to make a bad run LEGIBLE must never be the thing
// that makes it illegible. gui/vitest.config.ts registers it on every vitest
// invocation, including ones where gui/node_modules is absent, read-only, or
// out of space — so every artifact touch degrades to a warning, and the runner
// reads the resulting absent artifact as "do not retry".
describe('WorkerRpcFlakeReporter — IO failures degrade, never abort the run', () => {
  const exploding = (message: string) => () => {
    throw new Error(message)
  }

  it('survives a failing artifact write and warns instead', () => {
    const { reporter, lines, warnings } = recordingReporter('/tmp/flake.json', {
      writeArtifact: exploding('ENOSPC: no space left on device'),
    })

    expect(() => reporter.onTestRunEnd(starvedRun(), [])).not.toThrow()
    // The marker still reaches stdout: recognition does not depend on the disk.
    expect(lines).toHaveLength(1)
    expect(warnings.join('\n')).toContain('ENOSPC')
  })

  it('survives a failing stale-artifact discard at run start', () => {
    const { reporter, warnings } = recordingReporter('/tmp/flake.json', {
      discardArtifact: exploding('EACCES: permission denied'),
    })

    expect(() => reporter.onTestRunStart()).not.toThrow()
    expect(warnings.join('\n')).toContain('EACCES')
  })

  it('survives a failing discard on the negative path', () => {
    const { reporter, warnings } = recordingReporter('/tmp/flake.json', {
      discardArtifact: exploding('ENOENT: no such file or directory'),
    })

    expect(() => reporter.onTestRunEnd([testModule(`${ROOT}/a.test.ts`)], [])).not.toThrow()
    expect(warnings.join('\n')).toContain('ENOENT')
  })
})
