import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'

// Decides which gui test files may share a fork worker's module graph (task 7642).
// Fail-safe: a file this walk misses, or one that calls a module-registry vi API,
// runs in the fresh-modules project, so a gap costs speed, never correctness.
// Rationale and test-author rules: docs/notes/gui-vitest-shared-workers.md.

/** A vi call whose effect depends on modules being evaluated after it runs. */
const MODULE_REGISTRY_CALL =
  /\bvi\s*\.\s*(mock|doMock|unmock|doUnmock|resetModules|importMock|importActual)\s*\(/

export function needsFreshModuleGraph(testSource: string): boolean {
  return MODULE_REGISTRY_CALL.test(testSource)
}

const TEST_FILE_NAME = /\.test\.tsx?$/

function isSkippedDirectory(name: string): boolean {
  return name === 'node_modules' || name.startsWith('.')
}

function listTestFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) return isSkippedDirectory(entry.name) ? [] : listTestFiles(path)
    return entry.isFile() && TEST_FILE_NAME.test(entry.name) ? [path] : []
  })
}

/** Test files under guiRoot that may share a module graph: POSIX paths relative to guiRoot, sorted. */
export function listSharedModuleTestFiles(guiRoot: string): string[] {
  return listTestFiles(guiRoot)
    .filter((path) => !needsFreshModuleGraph(readFileSync(path, 'utf8')))
    .map((path) => relative(guiRoot, path).split(sep).join('/'))
    .sort()
}
