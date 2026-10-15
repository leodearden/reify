import { afterEach, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import {
  listSharedModuleTestFiles,
  needsFreshModuleGraph,
} from '../../vitest-project-partition'

describe('needsFreshModuleGraph', () => {
  it.each([
    ['mock', "vi.mock('../stores/engineStore')"],
    ['doMock', "vi.doMock('../stores/engineStore', () => ({}))"],
    ['unmock', "vi.unmock('../stores/engineStore')"],
    ['doUnmock', "vi.doUnmock('../stores/engineStore')"],
    ['resetModules', 'vi.resetModules()'],
    ['importMock', "await vi.importMock('../stores/engineStore')"],
    ['importActual', "await vi.importActual('../stores/engineStore')"],
  ])('is true for a file calling vi.%s', (_api, call) => {
    expect(needsFreshModuleGraph(`import { vi } from 'vitest'\n${call}\n`)).toBe(true)
  })

  it('tolerates whitespace around the dot and before the paren', () => {
    expect(needsFreshModuleGraph("vi.mock ('./x')")).toBe(true)
    expect(needsFreshModuleGraph("vi\n  .mock('./x')")).toBe(true)
  })

  it('is false for a file using only non-registry vi APIs', () => {
    const source = [
      'const spy = vi.fn()',
      "vi.spyOn(console, 'error')",
      'vi.useFakeTimers()',
      "vi.stubGlobal('fetch', spy)",
      'const hoisted = vi.hoisted(() => ({ value: 1 }))',
    ].join('\n')
    expect(needsFreshModuleGraph(source)).toBe(false)
  })

  it('requires a word boundary before vi', () => {
    expect(needsFreshModuleGraph("myvi.mock('./x')")).toBe(false)
  })

  it('counts a mention inside a comment, since a false positive only costs speed', () => {
    expect(needsFreshModuleGraph("// we used to vi.mock('./x') here\n")).toBe(true)
  })
})

describe('listSharedModuleTestFiles', () => {
  let root: string | undefined

  afterEach(() => {
    if (root !== undefined) rmSync(root, { recursive: true, force: true })
    root = undefined
  })

  const makeRoot = (files: Record<string, string>): string => {
    root = mkdtempSync(join(tmpdir(), 'vitest-partition-'))
    for (const [relativePath, source] of Object.entries(files)) {
      const absolutePath = join(root, relativePath)
      mkdirSync(dirname(absolutePath), { recursive: true })
      writeFileSync(absolutePath, source)
    }
    return root
  }

  const clean = "import { it } from 'vitest'\nit('works', () => {})\n"

  it('lists only clean test files, sorted, relative to the gui root with POSIX separators', () => {
    const guiRoot = makeRoot({
      'src/__tests__/clean.test.tsx': clean,
      'src/__tests__/mocked.test.ts': `vi.mock('../x')\n${clean}`,
      'src/__tests__/nested/deep.test.ts': clean,
      'packages/p/src/__tests__/pkg.test.ts': clean,
      'test/visual/v.test.ts': clean,
      'node_modules/dep/x.test.ts': clean,
      'src/helper.ts': clean,
    })

    expect(listSharedModuleTestFiles(guiRoot)).toEqual([
      'packages/p/src/__tests__/pkg.test.ts',
      'src/__tests__/clean.test.tsx',
      'src/__tests__/nested/deep.test.ts',
      'test/visual/v.test.ts',
    ])
  })

  it('leaves dot-directories and names outside *.test.ts(x) to fresh-modules', () => {
    const guiRoot = makeRoot({
      'src/__tests__/kept.test.ts': clean,
      '.cache/x.test.ts': clean,
      'src/.hidden/y.test.tsx': clean,
      'src/__tests__/a.spec.ts': clean,
      'src/__tests__/b.test.js': clean,
      'src/__tests__/c.test.mts': clean,
    })

    expect(listSharedModuleTestFiles(guiRoot)).toEqual(['src/__tests__/kept.test.ts'])
  })

  it('is empty for an empty root', () => {
    expect(listSharedModuleTestFiles(makeRoot({}))).toEqual([])
  })
})
