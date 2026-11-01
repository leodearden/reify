# KiCad ↔ Reify integration — discussion position (2026-10-29)

**Status:** position for discussion, not a PRD. Nothing here is decided; no tasks or PRDs were
filed. Produced by a `/team` session (lead on fable; evidence seats on sonnet; a fable critic
pass on the assembled draft, whose four blockers and counter-position are folded in below).
Brief: `~/.claude/spawn-briefs/2026-10-29-reify-kicad-integration-team.md`. Evidence and
provenance: Appendix C.

**Verified against:** reify main `ff417c7c9c` (CLI probes on `target/debug/reify`); KiCad
clone `/media/leo/data_lv_1/leo/src/kicad`, master `de087340` (2026-10-23) and `origin/10.0`;
task store via fused-memory on 2026-10-29.

---

## 0. The position in six sentences

1. The valuable integration is **mechanical-interface co-design**: the board outline, mounting
   holes, keepouts and connector frames come from one typed Reify description; KiCad keeps
   every electrical fact; the loop closes through files Reify *generates* or *reads*, never a
   file both tools write.
2. The recommended seam is **generate-only**: Reify emits a KiCad footprint
   (`enclosure_interface.kicad_mod`: Edge.Cuts outline, NPTH mounting pads, keepout zones)
   that the electrical engineer places once and refreshes from the library; everything else
   is **pull-and-verify** — `kicad-cli pcb export step` (components labelled by reference
   designator, which `step-assembly-import` reads) and `export pos`, with `reify check`
   failing loudly when a connector drifts from its port frame.
3. That shape needs **no lossless `.kicad_pcb` editor, no live link, no KiCad 11**, and
   almost all of it is substrate Reify is already building (STEP assembly import, the
   constrained sketch, `std.features.holes`, `LocatedPort` mating, clearance, `report --bom`).
4. For **off-the-shelf boards** (all of `printer_v01` today) no KiCad integration is needed:
   a vendor STEP plus a Reify board-envelope structure covers it, and that is the STEP-import
   PRD's job already.
5. **Verdict: stage 0 now (conventions, no Reify code); stage 1 (one small PRD) only if there
   are three or more custom boards in view; stage 2 (live IPC push, KiCad 11 headless, IDX)
   optional and later.** A fat tool bridge or a schematic engine is not worth building.
6. **Do not start stage 1 before** the multi-file compile programme (#5516 → #5517 → #5505) and
   the STEP-import provider wiring (#8057, #8059) land, and fix the silent-bad-export bugs
   first because handoff automation trusts exit codes.

---

## 1. Workflows for projects with mechanical + PCBA elements

State: **T** substrate exists today · **P** planned with live tasks · **U** unowned.

| # | Workflow | Reify needs | KiCad side | State |
|---|---|---|---|---|
| W1 | **Board envelope in the assembly** — place a vendor or custom board, check clearance and component height against the enclosure, see it in the viewport | STEP assembly import as a module; `sub … at` / `relate`; `min_clearance` (arity-3 exists; arity-2 is #5441), `no_undeclared_interference` (#5443). Height limits are a Reify check against imported component bodies, because KiCad 10 rule areas carry no height | `kicad-cli pcb export step --subst-models --no-dnp` (10.0), or a vendor STEP | P: import #8055–#8066; clearance #5435/#5441/#5443; placement T |
| W2 | **Outline, holes, keepouts from the enclosure** — Reify derives them and KiCad consumes them | 2D profile (sketch, or `polygon` today), `ClearanceHole`-class features, a typed `BoardMechanicalInterface` value, a `.kicad_mod` emitter as a build output occurrence | A footprint may legally carry Edge.Cuts shapes, NPTH pads and keepout zones (10.0 parser); "Update Footprints from Library" refreshes it. DXF/SVG graphics import is GUI-only in 10 and master, so a file Reify generates is the scriptable path | P: sketch #5508–#5510; holes #5453; emitter U |
| W3 | **Connector alignment (the co-design loop)** — connector frames are ports in Reify; KiCad's connector footprints must sit on them | `PinPort`/`LocatedPort` frames; `connect` on `LocatedPort` as a fasten-mate (#5440); a `reify check` that reads `export pos` and reports drift. Reify cannot create `J1` (footprints come from the schematic), only verify or guide it: the interface footprint can carry connector target outlines on a user layer | `kicad-cli pcb export pos` CSV; IPC `UpdateItems` later for push | P: #5440, #9323, #7881; check U |
| W4 | **Reify-generated 3D models for KiCad libraries** — parametric connectors, heatsinks, standoffs | STEP export with product names (today a flat compound) and a "one definition → footprint + STEP" pattern like KiCad's own generators | `(model path (offset)(scale)(rotate))`, `${KICAD10_3DMODEL_DIR}` | T export; naming U |
| W5 | **One BOM / cost / sourcing** | `reify report --bom` (works, text only — needs a machine-readable form), `Buy`/`Costed`, catalog `in` (#8355), multi-file report (#5518) | `kicad-cli sch export bom` CSV | T + P |
| W6 | **Fab/assembly handoff** — one command regenerates everything with hashes | `reify build` Mode B occurrences; export hygiene fixes (#7808, #7378, #7468, #6085) | `kicad-cli jobset run` (all exports + DRC/ERC + `special_execute`) | T + P |
| W7 | Revision control and review | nothing new (text files; `kicad-cli pcb diff` is master-only) | — | T |
| W8 | System-level wiring / nets in the language | a net/connector notion; `connect` is a generic port edge with no type check (#7881) | netlist export | U — a language question, not integration |
| W9 | LLM-authored schematic/layout inside the Reify project (the 2026-10-24 study) | sidecar write tools (#8951), thin tools + `kicad-cli erc/drc` as verifiers | kicad-cli; IPC API | P; mostly skills, not Reify code |
| W10 | Co-simulation with the board as a part | after W1 | STEP + pos | far |

Design for W1–W3 and W5–W6. `printer_v01` today exercises only W1 (Pi/x86 host, Argon and
Ioni drives, BTT-style breakout, load-cell probe — all off-the-shelf), W5 and W6; it models
**no custom PCB** (`GElectronics` has one parameter; the tool-dock connector is a placeholder
box with `n_elec_pins`). Custom boards are plausible (toolhead breakout, chamber sensor
breakout, SimpleMotion adapter) but not planned in the project doc.

---

## 2. Integration architecture

### 2.1 Principle: never write a file KiCad also writes

Reify is the source of truth for the mechanical interface of a board; KiCad for everything
electrical. The seam is one **typed value** (heuristic 12, structured data instead of
meaningful strings): `BoardMechanicalInterface` — outline profile, thickness, holes with kind
and size, keepout regions, connector frames by reference designator. Two boundary adapters
consume it (heuristic 9, deep module with a narrow interface; heuristic 13, each adapter file
stands alone): an **emitter** that writes a footprint, and a **checker** that reads KiCad's
exports. Reify's engine never parses KiCad text; the adapters sit at the edge.

### 2.2 The recommended seam (critic's counter-position, adopted)

- **Reify → KiCad, generate-only.** `reify build` emits
  `<board>_mech.kicad_mod`: Edge.Cuts outline, NPTH mounting pads, keepout zones, optional
  connector target outlines on a user layer. The EE places it once; KiCad's library refresh
  picks up every later change. No lossless `.kicad_pcb` editor, no two writers of one file,
  no "Board changed on disk" prompt (KiCad asks before reloading a file edited underneath it;
  it never reloads silently).
- **KiCad → Reify, pull-and-verify.** The board enters the assembly through
  `step-assembly-import` from `kicad-cli pcb export step --subst-models --no-dnp` (10.0 has
  the full option set; the exporter labels the board body `PCB` and each component by its
  reference designator, sanitised by the import PRD to `r1`, `j3` …). `export pos` gives
  connector poses; `reify check` compares them with the enclosure's `LocatedPort` frames and
  fails on drift. Height-limited keepouts are checked here, against imported bodies, since
  KiCad 10 cannot hold a height limit on a rule area.
- **No circular build.** The footprint is an output artefact; the board STEP is an input
  import keyed by content hash (#8059's freshness/locked rules apply unchanged). They are
  different files, so a build never regenerates its own import. The earlier draft's
  "write into `.kicad_pcb`, then re-import the STEP in the same build" would have.
- **Verification.** `kicad-cli pcb drc --format json --exit-code-violations
  --schematic-parity` and `sch erc` are the only programmatic rule checks in any KiCad
  version (no DRC/ERC RPC exists in 10 or master); they slot in as external checks.
- **BOM.** Merge `sch export bom` rows into `reify report --bom` once it has a
  machine-readable form.

**Rejected alternatives.** *Lossless `.kicad_pcb` editing* (the draft's first design): needs a
round-trip editor (`kiutils-rs`, one maintainer, unverified losslessness, or an in-repo
s-expression layer), creates a two-writer file and a reload prompt, and the circular build
above; keep only as a stage-2 option via the IPC API with undo. *KiCad owns the interface,
Reify pulls geometry only*: what everyone does today; loses the "connectors cannot drift"
guarantee unless the check in W3 exists anyway — which is why the check, not the writer, is
the load-bearing piece. *IDX/EDMD*: the right long-term idea (incremental, reviewable
proposals) but KiCad's MR imports a baseline only, "maybe v11 (2027)".

### 2.3 Later layers

- **L2 (optional, KiCad 11):** IPC push with `BeginCommit`/`EndCommit` undo for interactive
  placement; `kicad-cli api-server` (headless, master-only), `PlaceFootprintFromLibrary`,
  job RPCs (`RunBoardJobExport3D` …), `export stackup`/`diff`/`idf` (all master-only). Nothing
  in the recommended seam depends on 11.
- **L3 (agent):** the Reify sidecar once #8951 lands, plus `kicad-cli` as verifier. Thin
  tools, per the 18/18-vs-3/18 A/B evidence against fat catalogues. Electrical skills
  (`aklofas/kicad-happy`, SKiDL) stay outside Reify.

### 2.4 Build on / avoid

| Build on | Why |
|---|---|
| `kicad-cli` 10.x: step, pos, bom, drc, erc, jobset | headless, versioned, file-based; the only DRC/ERC path |
| `step-assembly-import` (#8055–#8066) | RefDes-labelled XDE tree arrives free; #8056's provider seam is already format-agnostic (the format gate is #8059) |
| KiCad's "one definition → footprint + STEP" library-generator pattern | model for Reify-authored component libraries (W4) |
| IPC API + `kicad-python` (MIT) for L2 | official, undo-integrated; headless only in 11 |

| Avoid | Why |
|---|---|
| SWIG `pcbnew` | removed in 11 |
| Konnect | archived 2026-10-26 (AGPL); successor closed source — corrects the 2026-10-24 study |
| Any 200–400-tool MCP bridge | A/B evidence; context cost; several AGPL |
| Writing `.kicad_sch` or `.kicad_pcb` from Reify | electrical truth belongs in KiCad; two-writer files |
| IDF v3 | GUI-only export in 10, no import, nobody ships IDF models |
| DXF/SVG as the outline channel | import is GUI-only; holes arrive as milled circles |

### 2.5 Shape of the Reify-side surface — **proposal, nothing here parses today**

```reify
structure def ToolheadBoard : Board {            // Board: stdlib, std.electronics? (Q6)
    param thickness : Length = 1.6mm
    sketch outline { … }                          // constrained-2d-sketch (#5508–#5510) syntax
    sub m1 : ClearanceHole(thread: M3, …) at …    // std.features.holes (#5453) signature
    port j1 : in PinPort { /* frame */ }          // ports are `port x : in Trait { … }`
    aux let no_tall_parts = region_under(heatsink, 4mm)   // Reify-side height check (invented)
    sub mech = KiCadInterfaceOutput(subject: self, path: "toolhead_mech.kicad_mod")  // Mode B occurrence
    sub layout = STEPInput(source: "toolhead.step")       // from kicad-cli, via step-assembly-import
}
```

---

## 3. Benefit estimate and verdict

**Tactical benefit** starts with the first custom board and scales with the number of board
revisions: no hand-transcribed outline or holes (forum failure modes: holes milled instead of
drilled, connectors drifting after an enclosure change), one BOM, one regenerate command,
DRC/ERC in the same gate as `reify check`. Unmeasured; order of hours per revision and a
removed class of silent mismatch, not a step-change in what can be designed. With zero custom
boards the benefit is nil beyond what the STEP-import PRD gives.

**Strategic benefit** is larger: Reify holds the mechatronic single point of truth, and the
LLM-authored-PCB workflow of the 2026-10-24 study gets a typed mechanical spec to anchor on —
the benchmarks show models succeed with structured representations plus verifiers and fail on
raw files, and `BoardMechanicalInterface` plus `kicad-cli drc` is that shape.

**Cost.** Stage 0: install KiCad 10.0.7 (Flatpak; not installed today) and a CLI wrapper;
adopt a `.kicad_jobset` per board and `drc`/`erc` in CI. Stage 1: one PRD — `Board`
structure, the interface value, the `.kicad_mod` emitter with a KiCad-parser round-trip test,
the pos-based drift check, a machine-readable BOM. Comparable to a mid-size v0_6 PRD; most of
it is substrate under construction anyway.

**Verdict: stage 0 now; stage 1 iff three or more custom boards are in view; stage 2 later.**
Up-changers: a committed custom-board list for the printer, or a board-centric second
project. Down-changers: no custom boards in the next year (stay at stage 0); KiCad 11
shipping IDX (then stage 2's push channel is IDX, not IPC).

---

## 4. Reify work-in-progress to wait for or coordinate with

Verified by `get_task` on 2026-10-29 (69 non-terminal tasks read; Appendix A).

**Wait for (hard blockers on the import side):**
- #5516 `compile_program` → #5517 multi-file `build`/`eval` → #5505 `alias.Name`. Today
  `import lib as l; l.Plate()` fails at `check` and `build` compiles one file (probed).
- #8057 string-path import grammar (in implementer routing), #8059 provider wiring /
  freshness / locked mode (owns the format gate `E_IMPORT_UNSUPPORTED_FORMAT`), #8060 GUI/LSP
  host policy. The `reblock_guard` on #8055/#8056/#8059 was repointed on 2026-10-18 to the XDE
  toolkits row of `native-deps.toml`, which is present — resolved, not a risk.

**Coordinate:**
- #8059 — ask that the format gate admit a second provider without a fork (heuristic 3).
- #8065 — carries the BOM row with source path and content hash; reuse that provenance shape.
- #5508/#5509/#5510 sketch (face-with-holes = board outline); anchoring to a frame is out of
  scope there.
- #5453 `std.features.holes`; gated on #5451/#5452 and #5426/#5428.
- #5440 `connect` on `LocatedPort` as a mate, #5436 world frames, #5435/#5441/#5443 clearance.
- #9323 typed `PortPath` before touching `connect.rs`; #7881 port-type check (Leo's ruling
  esc-7881-5) is what makes `PinPort` connections meaningful.
- #8355 `in` operator; #5391 standard-parts programme (its text names FR4 substrate).
- #5518 multi-file `report`.

**Fix first (automation trusts exit codes):** #7808 (`out.obj` gets STEP bytes, exit 0),
#7378 (`export_step` drops the `Transfer` status; header-only file, `Ok`), #7468 (empty-shape
guard STEP-only), #6085 (STL tolerance 1000× off), #6592 (constructor args never reach sub
placements — silent geometry/value disagreement that would bite board placement).

**GUI/MCP:** #8951 (sidecar-reachable `reify_*` tools; #5101 is only the INV-GUI-3 lint),
#9387 `EntryDocument`, #8949 typed MCP request/response contract for any new tool.

**Unowned:** the `.kicad_mod` emitter, the pos-based drift check, machine-readable BOM, STEP
product structure on export (W4), any net/connector notion (W8).

---

## 5. Open questions for Leo, ordered by how much they change the plan

1. **Custom boards: which, and how many?** None → stage 0 only. Three or more → stage 1 now.
2. **Does Reify need to write into `.kicad_pcb` at all** — or is a generated `.kicad_mod`
   plus a pos/STEP-based `reify check` enough (this position says enough)? Three-way:
   generate a footprint / edit the board file / pull-and-verify only.
3. **Scope:** mechanical-interface co-design only, or also schematic/layout authoring inside
   Reify (the 2026-10-24 study)? The latter is mostly skills and `kicad-cli`; separate decision.
4. **KiCad 10 now or wait for 11?** The recommended seam needs nothing from 11; only stage 2
   does.
5. **Emitter inside `reify build`** (Mode B occurrence like `STEPOutput`) **or a separate
   `reify kicad` step?**
6. **Where does `Board` live** — stdlib under the standard-parts programme #5391, or `prj/`?
7. **Is W8 (typed nets/connectors in the language) wanted at all?** Independent of KiCad.

---

## Appendix A — WIP table pointer

The seat's full table (69 tasks: id, title, status, PRD, seam, why, blocked-by) is in the
session transcript; §4 lists every id that bears on the plan. Status vocabulary observed:
pending, deferred (plus done/cancelled dropped). Non-terminal = not done/cancelled.

## Appendix B — Corrections to the 2026-10-24 study's inputs

- Konnect archived 2026-10-26; drop from build-on.
- `kicad-cli api-server` is master-only (absent in `origin/10.0`); 9/10 headless IPC still
  means Xvfb.
- 10.0's `pcb export step` already has `--subst-models`, `--no-dnp`, `--component-filter`,
  `--net-filter`, `--board-only`, formats step/stepz/brep/xao/glb/ply/stl/u3d, and labels
  components by RefDes — a stronger MCAD channel than assumed.
- DRC/ERC: no RPC in master either; `kicad-cli` only.
- `export stackup`, `pcb diff`, `export idf` via CLI: master-only.

## Appendix C — Evidence and provenance

- Reify map (sonnet/medium, 196k tokens, 41 min): ten seams, `path::symbol` cites, three CLI
  probes. Stale PRD headers found: `io-lifecycle-bom-cost` (says DEFERRED; `report --bom`
  ships), `io-export-import-completion` §1, `sub-placement-and-surfacing` §9,
  `ai-native-editing` (8/9 landed).
- WIP scan (sonnet/medium, 387k tokens, 62 min): 16 `search_tasks` queries, 17 `prd_path`
  lookups, 90 `get_task` reads.
- Prior art (sonnet/medium, 128k tokens, 17 min): GitHub API reads 2026-10-29; KiCad docs
  9.0/10.0; FOSDEM 2026 IDX slides; KiCad MR !2341.
- KiCad ground truth (lead inline; the sonnet seat hit a rate limit): `kicad/cli/*` listing
  in master vs `origin/10.0`, `command_pcb_export_3d.cpp`, `command_api_server.cpp`,
  `command_pcb_import.cpp` (foreign-EDA, not graphics), `pcbnew/import_gfx/` (GUI only),
  `api/proto/board/{board_commands,board_types,board_jobs}.proto`,
  `api/proto/common/commands/editor_commands.proto`,
  `pcbnew/exporters/step/step_pcb_model.cpp` (RefDes labels), `pcbnew/cross-probing.cpp`
  (reload prompt), `common/jobs/` registry.
- Critic (fable/high, 171k tokens, 23 min): additionally verified `zone_settings.h`/`zone.h`/
  `drc_rule.h` (no rule-area height in 10.0), `pcb_io_kicad_sexpr_parser.cpp` (footprints
  accept `zone`, NPTH pads, Edge.Cuts), `pcb_actions.cpp` (update footprints from library),
  #5101 vs #8951, the reblock_guard repoint, `min_clearance` arity-3 in `units.rs`.
- Not verified: KiCad 11 release date; IDX timing beyond the slides; `export pos` CSV
  defaults; "hours per revision" is an estimate, not a measurement.
