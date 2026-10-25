import type { ResolvedConfig, TestProject } from 'vitest/node'
import { assignTestProjects, toGuiRelativePath } from './vitest-project-partition'

// Global setup (task 7642): fails the run unless vitest runs every gui test file
// in exactly the project vitest-project-partition.ts assigns it. vitest builds
// each project's file list by merging its include/exclude with the root config,
// so this is what catches a vitest upgrade that changes that merge, which would
// otherwise drop files silently or put a vi.mock file in shared-modules.
// See docs/notes/gui-vitest-shared-workers.md.

export interface ProjectFiles {
  readonly project: string
  /** Gui-relative paths of the test files vitest runs in this project. */
  readonly files: readonly string[]
}

export interface PartitionViolation {
  readonly file: string
  /** The project the partition assigns, or undefined when it assigns none. */
  readonly expected: string | undefined
  /** Every project vitest runs the file in, sorted. */
  readonly actual: readonly string[]
}

export function findPartitionViolations(
  expected: ReadonlyMap<string, string>,
  actual: readonly ProjectFiles[],
): PartitionViolation[] {
  const projectsByFile = new Map<string, string[]>()
  for (const { project, files } of actual) {
    for (const file of files) projectsByFile.set(file, [...(projectsByFile.get(file) ?? []), project])
  }
  const allFiles = [...new Set([...expected.keys(), ...projectsByFile.keys()])].sort()
  return allFiles.flatMap((file) => {
    const expectedProject = expected.get(file)
    const actualProjects = [...(projectsByFile.get(file) ?? [])].sort()
    const holds =
      expectedProject !== undefined && actualProjects.length === 1 && actualProjects[0] === expectedProject
    return holds ? [] : [{ file, expected: expectedProject, actual: actualProjects }]
  })
}

function describeViolation({ file, expected, actual }: PartitionViolation): string {
  const runsIn = actual.length === 0 ? 'no project' : actual.join(' and ')
  const belongs =
    expected === undefined ? 'assigned to no project by the partition' : `belongs in ${expected}`
  return `  ${file}: runs in ${runsIn}, ${belongs}`
}

export function formatPartitionViolations(violations: readonly PartitionViolation[]): string {
  return [
    'gui vitest projects disagree with gui/vitest-project-partition.ts ' +
      '(see docs/notes/gui-vitest-shared-workers.md):',
    ...violations.map(describeViolation),
  ].join('\n')
}

/** --project or --dir leave part of the partition unglobbed, so a narrowed run cannot check it. */
function scansWholePartition(config: ResolvedConfig): boolean {
  return config.project.length === 0 && !config.dir
}

export async function setup(project: TestProject): Promise<void> {
  const { vitest } = project
  if (!project.isRootProject() || !scansWholePartition(vitest.config)) return
  const guiRoot = vitest.config.root
  const actual = await Promise.all(
    vitest.projects.map(async (testProject) => ({
      project: testProject.name,
      files: (await testProject.globTestFiles()).testFiles.map((file) => toGuiRelativePath(guiRoot, file)),
    })),
  )
  const violations = findPartitionViolations(assignTestProjects(guiRoot), actual)
  if (violations.length > 0) throw new Error(formatPartitionViolations(violations))
}
