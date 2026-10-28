import { describe, expect, it } from 'vitest'
import {
  findPartitionViolations,
  formatPartitionViolations,
} from '../../vitest-partition-guard'

const expected = new Map([
  ['src/a.test.ts', 'shared-modules'],
  ['src/b.test.ts', 'fresh-modules'],
])

describe('findPartitionViolations', () => {
  it('finds none when each file runs in exactly its assigned project', () => {
    expect(
      findPartitionViolations(expected, [
        { project: 'shared-modules', files: ['src/a.test.ts'] },
        { project: 'fresh-modules', files: ['src/b.test.ts'] },
      ]),
    ).toEqual([])
  })

  it('reports a file that runs in no project', () => {
    expect(
      findPartitionViolations(expected, [
        { project: 'shared-modules', files: [] },
        { project: 'fresh-modules', files: ['src/b.test.ts'] },
      ]),
    ).toEqual([{ file: 'src/a.test.ts', expected: 'shared-modules', actual: [] }])
  })

  it('reports a file that runs in the wrong project', () => {
    expect(
      findPartitionViolations(expected, [
        { project: 'shared-modules', files: ['src/a.test.ts', 'src/b.test.ts'] },
        { project: 'fresh-modules', files: [] },
      ]),
    ).toEqual([{ file: 'src/b.test.ts', expected: 'fresh-modules', actual: ['shared-modules'] }])
  })

  it('reports a file that runs in both projects, naming them sorted', () => {
    expect(
      findPartitionViolations(expected, [
        { project: 'shared-modules', files: ['src/a.test.ts'] },
        { project: 'fresh-modules', files: ['src/b.test.ts', 'src/a.test.ts'] },
      ]),
    ).toEqual([
      { file: 'src/a.test.ts', expected: 'shared-modules', actual: ['fresh-modules', 'shared-modules'] },
    ])
  })

  it('reports a file the partition does not assign, sorted among the others', () => {
    expect(
      findPartitionViolations(expected, [
        { project: 'shared-modules', files: [] },
        { project: 'fresh-modules', files: ['sidecar/s.test.ts', 'src/b.test.ts'] },
      ]),
    ).toEqual([
      { file: 'sidecar/s.test.ts', expected: undefined, actual: ['fresh-modules'] },
      { file: 'src/a.test.ts', expected: 'shared-modules', actual: [] },
    ])
  })
})

describe('formatPartitionViolations', () => {
  it('names every violating file with where it runs and where it belongs', () => {
    const message = formatPartitionViolations([
      { file: 'src/a.test.ts', expected: 'shared-modules', actual: [] },
      { file: 'sidecar/s.test.ts', expected: undefined, actual: ['fresh-modules'] },
    ])

    expect(message.split('\n').slice(1)).toEqual([
      '  src/a.test.ts: runs in no project, belongs in shared-modules',
      '  sidecar/s.test.ts: runs in fresh-modules, assigned to no project by the partition',
    ])
  })
})
