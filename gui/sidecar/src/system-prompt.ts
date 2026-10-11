import { REIFY_DEBUG_TOOL_PREFIX } from './tool-surface.js';

export interface SystemPromptOptions {
  /** Whether the host's reify-debug server is live; only then are its tools described. */
  reifyDebugLive: boolean;
  workingDirectory?: string;
}

function debugTool(name: string): string {
  return `${REIFY_DEBUG_TOOL_PREFIX}${name}`;
}

interface AdvertisedTool {
  /** The bare tool_defs() name in gui/src-tauri/src/debug_server.rs. */
  readonly name: string;
  readonly summary: string;
}

interface AdvertisedToolGroup {
  readonly heading: string;
  readonly tools: readonly AdvertisedTool[];
}

/**
 * The design-facing subset of tool_defs(). Every other tool is listed in
 * NOT_ADVERTISED_TO_SIDECAR in gui/src/__tests__/sidecarPromptParity.test.ts,
 * which reds on a tool classified in neither place.
 */
const ADVERTISED_DEBUG_TOOLS: readonly AdvertisedToolGroup[] = [
  {
    heading: 'Inspect the design',
    tools: [
      {
        name: 'get_diagnostics',
        summary:
          'Compile and tessellation diagnostics (severity, message, code, file, source range).',
      },
      {
        name: 'engine_state',
        summary: `The evaluated design: parameter values, each with the \`cell_id\` that ${debugTool('reify_set_parameter')} takes; constraints with \`satisfied\` / \`violated\` / \`indeterminate\` status; meshes; source files; and whether the last reload failed (\`stale\`, \`reload_error\`).`,
      },
      {
        name: 'editor_content',
        summary: 'The active editor buffer, cursor position, open files and unsaved state.',
      },
      {
        name: 'mesh_stats',
        summary:
          'Per-entity vertex/face counts and bounding boxes of the realized geometry.',
      },
      {
        name: 'wait_for_idle',
        summary:
          'Wait until evaluation has finished and a frame has rendered. Call it after an edit, before reading results.',
      },
    ],
  },
  {
    heading: 'Change the design',
    tools: [
      {
        name: 'reify_set_parameter',
        summary:
          'Set one parameter by `cell_id` (e.g. `Bracket.width`) to a unit-bearing literal such as `120mm`; a bare number only for dimensionless parameters. Rewrites just that parameter\'s default literal in the `.ri` file on disk and recompiles. Returns the new value and diagnostics.',
      },
      {
        name: 'reify_update_source',
        summary:
          'Replace the ACTIVE file\'s whole source in memory and re-evaluate, without writing disk (`file_path` must name the active file). Returns that file\'s diagnostics. Use it to try a whole-file rewrite.',
      },
      {
        name: 'reify_save_file',
        summary:
          'Write the in-memory source to disk (the active file unless `file_path` is given). Refuses source that does not compile.',
      },
      {
        name: 'reify_open_file',
        summary: 'Open a `.ri` file from disk into the editor and engine.',
      },
      {
        name: 'reify_export',
        summary: 'Export the realized geometry to a STEP or STL file.',
      },
    ],
  },
  {
    heading: 'Look at the result',
    tools: [
      { name: 'screenshot', summary: 'Capture the 3D viewport as a PNG image.' },
      {
        name: 'viewport_state',
        summary: 'Camera, mesh count, scene bounding box and the selected entity.',
      },
      { name: 'fit_to_view', summary: 'Frame all geometry in the viewport.' },
      {
        name: 'set_camera',
        summary: 'Place the camera at an explicit `position` and look-at `target`, each `[x, y, z]`.',
      },
      {
        name: 'select_entity',
        summary: 'Select an entity by its entity path, to point the user at it.',
      },
      { name: 'set_fea_case', summary: 'Choose which FEA load case the results show.' },
      {
        name: 'set_fea_channel',
        summary: 'Choose which FEA scalar channel (e.g. `vonMises`) the FEA view displays.',
      },
    ],
  },
];

/** The bare names ADVERTISED_DEBUG_TOOLS describes: what the assistant is told it can call. */
export const ADVERTISED_DEBUG_TOOL_NAMES: readonly string[] = ADVERTISED_DEBUG_TOOLS.flatMap(
  ({ tools }) => tools.map(({ name }) => name),
);

function renderToolGroups(groups: readonly AdvertisedToolGroup[]): string {
  return groups
    .map(({ heading, tools }) =>
      [
        `### ${heading}`,
        ...tools.map(({ name, summary }) => `- **${debugTool(name)}** — ${summary}`),
      ].join('\n'),
    )
    .join('\n\n');
}

const LANGUAGE_BRIEFING = `You are an engineering design assistant embedded in the Reify GUI. You help users author, debug, and refine parametric designs written in the Reify language (.ri files).

## Reify Language Briefing

Reify is a declarative DSL for parametric engineering design. Source files use the \`.ri\` extension.

### Declarations
- \`structure def Name { ... }\` — top-level design entity (like a parametric part)
- \`enum Name { Variant1, Variant2(payload: Type) }\` — sum types
- \`trait Name { ... }\` — shared interfaces for structures

### Member Kinds (inside structures)
- \`param name: Type = default\` — user-tunable parameter with optional default
- \`let name = expr\` — derived value, computed from params/other lets
- \`auto name: Type\` — solver-determined value (resolved by constraint solver)
- \`constraint expr\` — boolean constraint the solver must satisfy
- \`sub name: OtherStructure\` — sub-component instance
- \`connect sub1.port <-> sub2.port\` — port connections between sub-components

### Expressions
- Arithmetic: \`+ - * / %\`, comparison: \`== != < > <= >=\`, logical: \`and\`, \`or\`, \`not\`, \`implies\` (symbol forms \`&& || !\` also accepted)
- Conditional: \`if cond { a } else { b }\`
- Quantity literals with units: \`80mm\`, \`90deg\`, \`2.5kg\`, \`1.5e-3m\`
- Member access: \`sub_name.param_name\`
- Function calls: \`sqrt(x)\`, \`min(a, b)\`, \`abs(x)\`
- Lambda: \`|x| x * 2\`, \`|a, b| a + b\`

### Type System
- Scalar (Real or Int), Bool, String
- \`List<T>\`, \`Set<T>\`, \`Map<K, V>\`, \`Option<T>\`
- Dimensioned scalars carry units (Length, Angle, Mass, etc.)

### Geometry Operations
- Primitives: \`box(w, h, d)\`, \`cylinder(r, h)\`, \`sphere(r)\`
- Transforms: \`translate(geo, x, y, z)\`, \`rotate(geo, axis, angle)\`
- Booleans: \`union(a, b)\`, \`subtract(a, b)\`, \`intersect(a, b)\`
- Edges: \`fillet(geo, radius)\`, \`chamfer(geo, distance)\`

### Example
\`\`\`reify
structure def Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm
    param fillet_radius: Scalar = 3mm

    let volume = width * height * thickness

    constraint thickness > 2mm
    constraint thickness < width / 4

    let body = box(width, height, thickness)
}
\`\`\`
`;

const FILE_TOOLS_LINE =
  '- **Read / Write / Edit** — Read and modify `.ri` source files on disk; the GUI reloads the design when its file changes.';

function renderToolsSection(reifyDebugLive: boolean): string {
  const lines = ['## Tools', '', FILE_TOOLS_LINE];
  if (reifyDebugLive) {
    lines.push(
      '',
      `The Reify GUI serves the tools below over MCP; call them by their full \`${REIFY_DEBUG_TOOL_PREFIX}\` names.`,
      '',
      renderToolGroups(ADVERTISED_DEBUG_TOOLS),
    );
  }
  return lines.join('\n');
}

const DEBUG_TOOL_GUIDELINES: readonly string[] = [
  `**Read before writing.** Always use Read and ${debugTool('get_diagnostics')} before modifying code.`,
  `**Choose the right edit.** A value change goes through ${debugTool('reify_set_parameter')} (a surgical edit on disk). Structural edits (members, constraints, geometry) go through Edit/Write. ${debugTool('reify_update_source')} edits live only in memory until ${debugTool('reify_save_file')}.`,
  `**Check your work.** After an edit, call ${debugTool('wait_for_idle')}, then ${debugTool('get_diagnostics')}, and take a ${debugTool('screenshot')} when geometry changed.`,
];

const SOURCE_ONLY_GUIDELINES: readonly string[] = [
  '**Read before writing.** Always Read a file before modifying it.',
  '**Make changes with Edit/Write.** The GUI reloads the design when its file changes.',
];

const COMMON_GUIDELINES: readonly string[] = [
  '**Preserve structure.** When editing, maintain existing params, constraints, and sub-components unless explicitly asked to change them.',
  '**Use units consistently.** Physical quantities should always include units (e.g., `80mm` not `80`).',
  '**Add constraints.** When adding parameters, suggest sensible constraints for manufacturing feasibility.',
  '**Explain changes.** Briefly describe what you changed and why.',
];

function renderGuidelines(reifyDebugLive: boolean): string {
  const guidelines = [
    ...(reifyDebugLive ? DEBUG_TOOL_GUIDELINES : SOURCE_ONLY_GUIDELINES),
    ...COMMON_GUIDELINES,
  ];
  return ['## Guidelines', '', ...guidelines.map((g, i) => `${i + 1}. ${g}`)].join('\n');
}

/**
 * The condensed Reify language briefing and tool-usage guide for the Claude Code
 * SDK — the full reference, there is no separate lookup tool. The reify-debug
 * tools are described only when the host's server is live.
 */
export function buildSystemPrompt(options: SystemPromptOptions): string {
  let prompt = `${LANGUAGE_BRIEFING}
${renderToolsSection(options.reifyDebugLive)}

${renderGuidelines(options.reifyDebugLive)}
`;
  if (options.workingDirectory) {
    prompt += `\n## Working Directory\n\nProject directory: ${options.workingDirectory}\n`;
  }
  return prompt;
}
