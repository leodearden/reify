import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, sep } from 'node:path'

// Decides which vitest project each gui test file runs in (task 7642): files
// that may share a fork worker's module graph, and files that need a fresh one.
// Fail-safe: a file that calls a module-registry vi API, or merely mentions one,
// runs in fresh-modules, so a false positive costs speed, never correctness.
// vitest-partition-guard.ts fails the run if vitest's projects disagree with this.
// Rationale and test-author rules: docs/notes/gui-vitest-shared-workers.md.

export const SHARED_MODULES_PROJECT = 'shared-modules'
export const FRESH_MODULES_PROJECT = 'fresh-modules'
export type TestProjectName = typeof SHARED_MODULES_PROJECT | typeof FRESH_MODULES_PROJECT

/** Directories directly under the gui root whose test files are not gui vitest files. */
export const NON_GUI_TEST_DIRECTORIES: readonly string[] = ['sidecar', 'node_modules']

/** A vi call whose effect depends on modules being evaluated after it runs. */
const MODULE_REGISTRY_CALL =
  /\bvi\s*\.\s*(mock|doMock|unmock|doUnmock|resetModules|importMock|importActual)\s*\(/

export function needsFreshModuleGraph(testSource: string): boolean {
  return MODULE_REGISTRY_CALL.test(testSource)
}

const TEST_FILE_NAME = /\.test\.tsx?$/

function isWalkedDirectory(guiRoot: string, path: string, name: string): boolean {
  if (name === 'node_modules' || name.startsWith('.')) return false
  return !NON_GUI_TEST_DIRECTORIES.includes(toGuiRelativePath(guiRoot, path))
}

function listTestFiles(guiRoot: string, directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) {
      return isWalkedDirectory(guiRoot, path, entry.name) ? listTestFiles(guiRoot, path) : []
    }
    return entry.isFile() && TEST_FILE_NAME.test(entry.name) ? [path] : []
  })
}

/** POSIX path of `path` relative to the gui root: the key every partition list uses. */
export function toGuiRelativePath(guiRoot: string, path: string): string {
  return relative(guiRoot, path).split(sep).join('/')
}

/** Every gui test file under guiRoot, keyed by its gui-relative path, with the project it must run in. */
export function assignTestProjects(guiRoot: string): Map<string, TestProjectName> {
  return new Map(
    listTestFiles(guiRoot, guiRoot).map((path) => [
      toGuiRelativePath(guiRoot, path),
      needsFreshModuleGraph(readFileSync(path, 'utf8')) ? FRESH_MODULES_PROJECT : SHARED_MODULES_PROJECT,
    ]),
  )
}

/** Test files under guiRoot that may share a module graph: gui-relative paths, sorted. */
export function listSharedModuleTestFiles(guiRoot: string): string[] {
  return [...assignTestProjects(guiRoot)]
    .filter(([, project]) => project === SHARED_MODULES_PROJECT)
    .map(([file]) => file)
    .sort()
}
