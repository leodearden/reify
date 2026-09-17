import { describe, it, expect } from 'vitest'
import {
  classifyWorkerRpcFlake,
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
          filepath: 'vitest.setup.ts',
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
      'vitest.setup.ts',
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
          filepath: 'vitest.setup.ts',
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
