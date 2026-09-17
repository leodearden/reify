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
): WorkerRpcFailureSummary => ({ failedSuites, failedTestCount })

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

  it('returns null when no suites failed at all', () => {
    expect(classifyWorkerRpcFlake(summary([]))).toBeNull()
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

/** A synthetic stand-in for vitest's TestModule, satisfying ReportedModule. */
const testModule = (
  moduleId: string,
  opts: { failed?: boolean; errors?: string[]; failedTests?: number } = {},
): ReportedModule => ({
  moduleId,
  state: () => (opts.failed ? 'failed' : 'passed'),
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
}

const recordingReporter = (artifactPath = '/tmp/flake.json'): Recorder => {
  const lines: string[] = []
  const writes: { path: string; contents: string }[] = []
  const discards: string[] = []
  const reporter = new WorkerRpcFlakeReporter({
    rootDir: ROOT,
    artifactPath,
    emit: (line) => lines.push(line),
    writeArtifact: (path, contents) => writes.push({ path, contents }),
    discardArtifact: (path) => discards.push(path),
  })
  return { reporter, lines, writes, discards }
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

describe('WorkerRpcFlakeReporter — marker line', () => {
  it('emits exactly one column-0 @@REIFY_GUI_FLAKE@@ line on the positive signature', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines).toHaveLength(1)
    expect(lines[0].startsWith('@@REIFY_GUI_FLAKE@@ ')).toBe(true)
  })

  it('carries kind, suite count and the method csv in the established key=value grammar', () => {
    const { reporter, lines } = recordingReporter()
    reporter.onTestRunEnd(starvedRun(), [])

    expect(lines[0]).toContain('kind=worker_rpc_timeout')
    expect(lines[0]).toContain('suites=2')
    expect(lines[0]).toContain('methods=fetch,snapshotSaved')
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

  it('defaults the artifact path under the gui root when none is injected', () => {
    expect(WORKER_RPC_FLAKE_ARTIFACT).toBe('node_modules/.reify-gui-rpc-flake.json')
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
