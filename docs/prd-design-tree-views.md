# PRD: Design Tree & View System

**Version:** 0.1
**Date:** 2026-04-15
**Author:** Leo (with Claude)
**Status:** Draft
**Depends on:** M6 "Visual" GUI (complete)

---

## 1. Problem Statement

The GUI renders every `Solid` it finds with no distinction between result geometry and construction intermediates. A `BoltFlange` with four `let` bindings (`body`, `hole`, `holes`, `flange`) renders all four solids in the viewport — the user cannot tell which is the "real" flange and which are intermediate CSG steps. There is no way to hide, ghost, or selectively display geometry.

For simple designs this is merely cluttered. For complex assemblies it is unusable: every intermediate of every sub-structure is visible simultaneously, the viewport becomes a soup of overlapping solids, and the designer cannot focus on what matters.

The `Physical` trait provides a semantic answer (`param geometry : Solid` designates the result), but the GUI has no mechanism to consume this information or let the user control what they see.

### 1.1 Secondary Problems

**No design hierarchy visibility.** The GUI has no tree view of the entity structure. The PropertyEditor groups parameters by entity name prefix, but doesn't show the containment hierarchy (parent structures, sub-structures, ports, geometric bindings). Engineers coming from any CAD tool expect a feature/assembly tree.

**No view persistence.** Any visibility state the user sets up must be reconstructed from scratch after every re-evaluation or session restart.

**No multi-viewport.** A single viewport conflates two distinct modes: authoring a definition (where you want to see a prototype of the thing you're editing) and composing a design (where you want to see instantiated assemblies with controlled visibility). These modes have different needs and will inevitably require independent viewports.

---

## 2. Design Principles

### 2.1 Presentation/Design Boundary

Presentation and design are separate domains with a strict dependency direction:

```
Presentation ──references──> Design
Design ──never references──> Presentation
```

The Reify language (`.ri` files) describes engineering design: structures, constraints, geometry, materials, occurrences. The presentation layer describes how to visualize it: which entities to show, how to render them, camera positions, view configurations.

This is analogous to CSS and HTML. The design is the document; presentation is the stylesheet. Presentation state lives outside `.ri` files. Purpose declarations control design semantics (constraints, optimization), not views — though the presentation layer may observe active purposes and generate appropriate views in response.

A presentation language/spec may come in due course, but for now, presentation state is UI state persisted in sidecar files and localStorage.

### 2.2 View as Overlay

A "view" is a named mapping from entity paths to visibility states, overlaid on the 3D scene. Views reference the design graph but don't modify it. Multiple views can exist for the same design. Switching views is a presentation concern — the engine doesn't know or care which view is active.

### 2.3 Instance-Centric with Def Preview

The primary tree shows the **instance structure** (the elaborated evaluation graph), not the definition structure (what's in the `.ri` source). This is what matters for assemblies: you see the actual things that exist, not the templates they came from.

However, when authoring a definition, you need visual feedback without ritually instantiating it. A separate **def preview viewport** shows a prototype of the selected definition with all defaults applied. This is a tool affordance, not part of the evaluation graph.

---

## 3. Architecture

### 3.1 Component Overview

```
                    ┌─────────────────────────────────────────────┐
                    │              Presentation Layer              │
                    │                                             │
  ┌──────────┐     │  ┌───────────┐  ┌────────────┐  ┌────────┐ │
  │ Design   │     │  │ View      │  │ Design     │  │ View   │ │
  │ Tree     │◄────┤  │ State     │  │ Tree       │  │ Mgmt   │ │
  │ Panel    │     │  │ Store     │  │ Store      │  │ UI     │ │
  │          │────►│  │           │  │            │  │        │ │
  └──────────┘     │  └─────┬─────┘  └─────┬──────┘  └───┬────┘ │
                    │        │              │              │      │
                    │        ▼              │              │      │
  ┌──────────┐     │  ┌─────────────┐      │              │      │
  │ Viewport │◄────┤  │ Viewport    │◄─────┘              │      │
  │ (Three)  │     │  │ Controller  │                     │      │
  └──────────┘     │  └─────────────┘                     │      │
                    │        ▲                             │      │
                    │        │                             │      │
                    │  ┌─────────────┐  ┌────────────────┐│      │
                    │  │ Persistence │  │ Auto View      ││      │
                    │  │ (local +    │  │ Generator      ├┘      │
                    │  │  sidecar)   │  │                │       │
                    │  └─────────────┘  └───────┬────────┘       │
                    │                           │                │
                    └───────────────────────────┼────────────────┘
                                                │
                    ┌───────────────────────────┼────────────────┐
                    │         Engine Layer       │                │
                    │                           ▼                │
                    │  ┌────────────┐  ┌────────────────┐        │
                    │  │ Entity     │  │ Trait / Purpose │        │
                    │  │ Graph      │  │ Metadata        │        │
                    │  └────────────┘  └────────────────┘        │
                    └────────────────────────────────────────────┘
```

### 3.2 New Stores

**`designTreeStore`** — SolidJS store holding the hierarchical entity tree derived from the engine's evaluation graph. Rebuilt on re-evaluation. Each node carries:
- `entityPath: string` — dot-separated path (e.g., `"Assembly.housing.bore_cutout"`)
- `kind: "structure" | "sub" | "let" | "param" | "port"` — declaration kind
- `typeName: string | null` — e.g., `"Solid"`, `"BoltFlange"`, `"List<Vent>"`
- `hasMesh: boolean` — whether the engine produced renderable geometry for this node
- `traitGeometry: boolean` — whether this is the `geometry` param from `Physical`
- `children: TreeNode[]`

**`viewStateStore`** — SolidJS store holding the current view configuration. Decoupled from the design tree (multiple views can exist for the same tree):
- `viewId: string` — current active view identifier
- `views: Map<string, ViewDefinition>` — all available views (auto + user)
- `ViewDefinition`:
  - `name: string`
  - `auto: boolean` — whether this was system-generated
  - `modified: boolean` — COW flag (auto view that user has edited)
  - `visibility: Map<string, VisibilityState>` — per-entity-path overrides
  - `defaults: VisibilityDefaults` — rules for entities without explicit overrides

**`viewportStore`** — SolidJS store managing viewport instances and their view assignments:
- `viewports: Map<string, ViewportState>` — viewport ID to state
- `ViewportState`:
  - `type: "design" | "def-preview"`
  - `viewId: string | null` — which view this viewport shows (design viewports)
  - `defPath: string | null` — which definition to preview (def-preview viewports)
  - `camera: CameraState`
  - `active: boolean` — is this the focused viewport?

### 3.3 Backend Support (New Tauri Commands)

```rust
#[tauri::command]
fn get_entity_tree() -> EntityTree;
// Returns the hierarchical entity structure of the current evaluation graph.
// Each node: entity_path, kind, type_name, has_mesh, trait_geometry flag, children.

#[tauri::command]
fn get_def_preview(def_path: String) -> GuiState;
// Returns a GuiState (meshes, values, constraints) for a prototype instance
// of the named definition with all defaults applied. Does not affect the
// main evaluation graph.

#[tauri::command]
fn get_entity_identity_map() -> Map<String, EntityIdentity>;
// Returns stable identity hints for entities in the current evaluation.
// EntityIdentity includes a content hash and structural fingerprint
// for fuzzy matching across re-elaborations.
```

---

## 4. Design Tree Panel

### 4.1 Location and Layout

New panel on the right side, tabbed with the existing PropertyEditor. Two tabs: **Tree** (design tree with view controls) and **Properties** (existing PropertyEditor).

The tree tab shows a collapsible hierarchy of the design. Each node has:
- Expand/collapse chevron (for nodes with children)
- Icon indicating kind (structure, geometry solid, geometry intermediate, port, etc.)
- Name label (entity name, not full path)
- Type label (dimmed, e.g., `Solid`, `BoltFlange`)
- Visibility toggle (eye icon, cycles through states on click)

### 4.2 Tree Content

The tree shows **all entities that have geometric presence** in the evaluation graph, plus their parent containers:

```
Assembly                        [structure]
  geometry                      [param Solid — from Physical]
  housing                       [sub Housing]
    geometry                    [param Solid — from Physical]
    bore_cutout                 [let Solid — intermediate]
  flange                        [sub BoltFlange]
    geometry                    [param Solid — from Physical]
    body                        [let Solid — intermediate]
    hole                        [let Solid — intermediate]
    holes                       [let List<Solid> — intermediate]
```

**Inclusion rules:**
- Structures and sub-structure instances: always shown (they're containers)
- `param geometry` from `Physical` trait: always shown, marked as primary
- `let` bindings of geometric type (`Solid`, `Surface`, `Curve`): shown under their parent
- `param` of geometric type (other than `geometry`): shown
- Non-geometric members (scalar params, constraints, etc.): not shown in tree (they live in PropertyEditor)
- Ports: shown if they have geometric significance (future consideration)

### 4.3 Interaction

**Click:** Select entity. Highlights in viewport, populates PropertyEditor, optionally scrolls editor to source.

**Double-click:** Fly viewport camera to entity's bounding box.

**Eye icon click:** Cycle visibility: show -> ghost -> hidden -> show. Visual indicator changes with state.

**Right-click:** Context menu:
- Show only this
- Show this and children
- Ghost this and children
- Hide this and children
- Reset to view defaults

**Ctrl+click:** Toggle entity in/out of multi-selection.

**Shift+click:** Range select (all nodes between last click and this click).

**Multi-selection + eye icon:** Apply visibility change to all selected nodes.

**Drag (future):** Reorder is not meaningful (tree reflects design structure, not user preference). No drag support.

---

## 5. Visibility Model

### 5.1 States

| State | Rendering | Selectable | Opacity | Edge display |
|-------|-----------|------------|---------|--------------|
| **Show** | Full material color | Yes | 1.0 | Normal |
| **Ghost** | Transparent, single flat color | No | 0.12-0.18 | None |
| **Hidden** | Not rendered | No | N/A | N/A |

Ghost meshes render in a dedicated pass (after opaque, before transparent UI overlays) with depth-write disabled and a neutral color (e.g., Catppuccin `surface0`). They provide spatial context without competing for attention.

### 5.2 Cascading

Visibility changes **cascade recursively** to all children by default. The mental model: a thing is composed of its members; hiding the thing hides everything inside it.

```
User hides "housing" →
  housing: hidden
    housing.geometry: hidden
    housing.bore_cutout: hidden
```

**Override:** A child's visibility can be set independently after a cascade. The child's explicit state takes precedence over any future parent cascade — until the child's override is cleared (reset to "inherit").

Implementation: each node stores either an explicit `VisibilityState` or `inherit`. `inherit` means "use parent's state." Explicit states are overrides. The cascade is computed, not stored — changing a parent recomputes all `inherit` descendants.

### 5.3 Effective Visibility Resolution

```
effective_visibility(node):
    if node.explicit_state != inherit:
        return node.explicit_state
    if node.parent:
        return effective_visibility(node.parent)
    return view.default_state
```

### 5.4 Default Rules

When no explicit visibility is set (fresh view, new entities), the **auto view generator** applies these rules:

1. If the structure implements `Physical` (has `param geometry : Solid`):
   - `geometry`: **show**
   - All other `Solid`/`Surface`/`Curve` let-bindings: **hidden**
2. If the structure does not implement `Physical`:
   - All geometric bindings: **show**
3. Sub-structure instances: **show** (recurse into their own rules)
4. Non-geometric entities: not in tree, not rendered (moot)

This means the BoltFlange example, once it has `: Rigid` and `param geometry`, will show only the final solid by default. Toggle the intermediates on via the tree when you want to debug the CSG.

---

## 6. View Management

### 6.1 Auto Views

The system generates one or more views automatically based on the design structure. Auto views update dynamically as the design changes (entities added/removed).

**Guaranteed auto views:**
- **"Default"** — applies the rules from Section 5.4. Always exists. Cannot be deleted.

**Conditional auto views (generated when applicable):**
- **"All geometry"** — show everything geometric. Useful for debugging.
- **Purpose-linked views** — when a `purpose` is active, generate a view that shows entities relevant to that purpose (e.g., `manufacturing_ready` → show final geometry + material annotations, hide intermediates and analysis scaffolding). The mapping from purpose to view is heuristic, not declaratively specified.

Auto views display with a system icon (e.g., gear or magic wand) in the view selector to distinguish them from user views.

### 6.2 Named Views (User-Created)

Users can create named views from scratch via a "New view" button. A named view stores explicit visibility state for every entity the user has touched, plus a default rule for entities not explicitly set.

View management UI: a dropdown/selector at the top of the Design Tree panel showing the active view, with:
- List of all views (auto + user)
- Active view indicator
- New view button
- Rename (user views only)
- Delete (user views only)
- Duplicate

### 6.3 Copy-on-Write

When the user modifies visibility in an auto view, the system:

1. Creates a copy of the auto view with name `"Default (modified)"` (or similar)
2. Applies the user's change to the copy
3. Switches the active view to the copy
4. The original auto view remains unchanged and continues to update with the design

The user can rename the copied view. If they discard it, the auto view is still there.

This means auto views are always "clean" — they reflect the system's best guess. User modifications are preserved but never pollute the auto-generated state.

### 6.4 View Selector Interaction

```
┌─ View: Default ─────────────────── ▾ ┐
│                                       │
│  ⚙ Default                     ✓     │
│  ⚙ All geometry                       │
│  ⚙ manufacturing_ready                │
│  ─────────────────────────────        │
│  ◆ My assembly view                  │
│  ◆ Default (modified)                │
│  ─────────────────────────────        │
│  + New view                           │
│  ⊘ Manage views...                   │
└───────────────────────────────────────┘
```

`⚙` = auto view, `◆` = user view, `✓` = active.

---

## 7. Dual Viewport

### 7.1 Rationale

Two distinct modes of work need different viewports:

**Definition authoring:** "I'm writing `BoltFlange`. Show me what it looks like with defaults applied." The viewport shows a single prototype instance, automatically updated as the definition changes. No assembly context, no sub-structure nesting — just the thing being defined.

**Design composition:** "I'm building an assembly from multiple structures. Show me the instances I've placed, with controlled visibility." The viewport shows the instantiated design with the active view applied.

These are fundamentally different: one shows a preview of a template, the other shows the real thing.

### 7.2 Layout

Two viewport regions in the center panel area, with adaptive layout:

| State | Layout |
|-------|--------|
| Both active | Side-by-side (horizontal split) or stacked (vertical split), user-adjustable |
| Only design active | Design viewport fills center, def preview minimized to a strip/tab |
| Only def preview active | Def preview fills center, design viewport minimized |
| Neither active | Placeholder with "Open a .ri file or select a definition" message |

**When is each active?**
- **Def preview:** Active when the editor cursor is inside a `structure def` or `occurrence def` body. The preview shows a prototype of that definition. Greyed out and minimized when the cursor is outside any definition, or when no file is open.
- **Design viewport:** Active when the evaluation graph contains at least one instantiated entity (at least one `sub` instantiation exists in the loaded files, or a top-level structure is being evaluated). Greyed out and minimized when there are no instances.

### 7.3 Def Preview Behavior

The def preview viewport:
- Shows a single prototype instance of the selected definition
- Uses all param defaults (or `undef` display for unset params)
- Applies the "Default" auto view rules (show `geometry`, hide intermediates — if `Physical` is implemented)
- Has its own independent camera state (orbit/pan/zoom)
- Does NOT use named views or user view state — it always shows the auto default
- Updates live as the definition is edited
- Shows a label: "Preview: BoltFlange" in the viewport corner

The engine provides this via the `get_def_preview` command, which creates a temporary prototype without affecting the main evaluation graph.

### 7.4 Design Viewport Behavior

The design viewport:
- Shows the instance tree from the main evaluation graph
- Applies the active view from the view management system
- Has its own independent camera state
- Supports all interaction (selection, hover, navigation)
- Is the "primary" viewport for most workflows

---

## 8. View Persistence

### 8.1 Storage Layers

Two layers, with localStorage as the fast/ephemeral layer and sidecar files as the durable layer:

**Layer 1: localStorage (always active)**
- Stores the current view state per file path
- Fast read/write, no file I/O
- Lost on browser cache clear or machine change
- Keyed by absolute file path of the primary `.ri` file

**Layer 2: Sidecar file (opt-in, durable)**
- File: `<filename>.ri.views.json` alongside the `.ri` file
- Written when the user explicitly saves views ("Save views" action) or on clean exit
- Version-controllable, shareable
- Contains: all named (user) views, active view selection, per-viewport camera state
- Auto views are NOT saved (they're generated from the design)

**Load priority:** sidecar file > localStorage > auto-generated defaults.

### 8.2 Entity Path Matching

Views reference entities by path (e.g., `"Assembly.flange.geometry"`). Paths can change when the design is refactored (renames, restructuring). The persistence system must handle this gracefully.

**Exact match (primary):** On re-evaluation, match stored entity paths against the new entity tree. Exact matches apply immediately.

**Fuzzy recovery (secondary):** For stored paths that don't match exactly, attempt recovery:
1. **Suffix match:** If `"Assembly.flange.geometry"` doesn't exist but `"Assembly.bolt_flange.geometry"` does, and it's the only candidate with matching suffix `".geometry"` under a sibling rename, suggest re-binding. (Heuristic, not automatic — notify user.)
2. **Structural match:** If the entity at a path has the same type, same parent type, and similar children, treat it as a likely rename. Again, suggest rather than auto-apply.
3. **Stale entries:** Paths that don't match after fuzzy recovery are kept in the view as "stale" (greyed out in the view management UI). They're not deleted — if the path returns (e.g., undo/redo, branch switch), the stored state is restored automatically.

**State restoration on path return:** If an entity path disappears (e.g., user renames `flange` to `plate`) and later returns (undo, or rename back), the stored visibility state for that path is automatically restored. This is free — the view stores state by path, and a returning path simply matches again.

### 8.3 Re-Evaluation Behavior

When the engine re-evaluates (source edit, param change):

1. New entity tree arrives from `get_entity_tree()`
2. Design tree store rebuilds
3. View state store reconciles:
   - Existing paths that still exist: keep their visibility state
   - New paths (new entities): apply default rules from active view
   - Missing paths (removed entities): mark as stale, keep in view state (for restoration)
4. Viewport re-renders with reconciled state

No visibility state is lost on re-evaluation. The user's view setup is stable.

---

## 9. Multi-Viewport Architecture

### 9.1 Abstraction

Viewports and views are decoupled. A viewport is a rendering surface with a camera; a view is a visibility configuration. Multiple viewports can show the same view (same visibility, different camera angles) or different views (different visibility, different cameras).

```
Viewport  ──uses──>  View  ──references──>  Entity Graph
Viewport  ──owns──>  Camera State
```

This decoupling is designed in from the start even though v0.1 ships with exactly two viewports (def preview + design). The abstraction boundary ensures that adding split viewports, picture-in-picture, or linked views later doesn't require architectural changes.

### 9.2 Viewport Identification

Each viewport has a stable ID and a type:
- `"design-main"` — the primary design viewport
- `"def-preview"` — the definition preview viewport
- Future: `"design-split-1"`, `"design-split-2"`, etc.

Viewport state (camera position, active view, etc.) is stored per viewport ID and persisted independently.

### 9.3 Future: Split Design Viewports

When split viewports are added, they will:
- Share the same entity graph
- Each have their own view assignment (can show different views)
- Each have their own camera state
- Support linked cameras (optional: orbit one, the other follows)

This is out of scope for v0.1 but the architecture supports it without changes to the view/viewport stores.

---

## 10. Multi-Selection

### 10.1 Selection Model

Extend the current single-selection model to support multi-selection:

- **Click:** Select single entity (deselects all others)
- **Ctrl+click:** Toggle entity in/out of selection set
- **Shift+click (in tree):** Range select (all visible nodes between anchor and target)
- **Ctrl+A (in tree):** Select all visible nodes
- **Escape:** Clear selection

### 10.2 Multi-Selection + Visibility

When multiple entities are selected and the user changes visibility (via eye icon, right-click menu, or keyboard shortcut):
- The change applies to all selected entities
- Cascading applies independently to each selected entity's subtree

### 10.3 Viewport Multi-Selection

- **Click:** Select the entity under the cursor (single select)
- **Ctrl+click:** Add/remove entity from selection set
- **Box select (future):** Drag to select all entities within a rectangular region

Multi-selection in the viewport and tree are synchronized — selecting in one updates the other.

### 10.4 Visual Feedback

- Selected entities: wireframe overlay (existing behavior, extended to multiple)
- Multi-selected entities: wireframe overlay on all selected
- Ghost entities: not selectable (click passes through)

---

## 11. Ghost Rendering

### 11.1 Three.js Implementation

Ghost meshes use a separate material and render pass:

```
Ghost material:
  - MeshBasicMaterial (no lighting response)
  - color: theme surface color (neutral, not per-entity)
  - transparent: true
  - opacity: 0.12-0.18 (tunable, possibly per-user preference later)
  - depthWrite: false
  - side: FrontSide (no backface — reduces visual noise)
```

Ghost meshes are added to a separate Three.js group (`ghostGroup`) that renders after the opaque group but before UI overlays (grid, axes, selection wireframes).

### 11.2 Interaction

Ghost meshes are excluded from raycasting. Clicks pass through them to opaque geometry behind, or to empty space. Hover tooltips are not shown for ghost meshes.

### 11.3 Performance

Ghost meshes use the same vertex/index buffers as their opaque counterparts — no additional geometry data. Only the material differs. Switching an entity between show and ghost is a material swap, not a geometry rebuild.

Hidden entities are removed from the scene entirely (not just invisible). This saves draw calls and raycasting cost.

---

## 12. Engine Integration

### 12.1 Entity Tree Emission

The backend must provide a structured entity tree (not just flat entity paths). This requires a new Tauri command (`get_entity_tree`) that walks the evaluation graph and emits:

- Hierarchical entity structure matching the instance tree
- Per-node metadata: kind, type, has_mesh flag, trait_geometry flag
- Identity hints for persistence (content hash, structural fingerprint)

This tree is rebuilt on every re-evaluation and sent to the frontend as a delta (or full replacement if the structure changed).

### 12.2 Def Preview

The `get_def_preview` command creates a temporary evaluation scope:
1. Instantiate the named definition with all defaults applied
2. Evaluate to produce geometry
3. Tessellate
4. Return `GuiState` (meshes + values + constraints)
5. Discard the temporary scope (no effect on main evaluation)

This must be lightweight — it runs on every cursor move between definitions. Caching by definition content hash is essential.

### 12.3 Selective Tessellation (Future Optimization)

Currently the engine tessellates all solids. With view control, hidden entities don't need tessellation. A future optimization:
- Frontend sends the set of visible entity paths to the backend
- Backend skips tessellation for hidden entities
- On visibility change (hidden -> show), backend tessellates on demand

This is **not** in the initial implementation — all entities are still tessellated, and visibility is controlled purely in the frontend by adding/removing meshes from the Three.js scene. The optimization is deferred until complex assemblies make it necessary.

### 12.4 Entity Identity Hints

To support fuzzy persistence matching (Section 8.2), the backend provides identity hints:
- **Content hash:** Hash of the entity's definition body (stable across renames)
- **Structural fingerprint:** Type name + parent type + child count + child types (stable across minor refactors)
- **Source span:** File + line range (for "jump to source" and structural correlation)

These hints are advisory — the frontend uses them for fuzzy matching but never trusts them blindly. Ambiguous matches prompt the user.

---

## 13. Detailed Requirements

### 13.1 Design Tree Panel

| ID | Requirement | Priority |
|----|-------------|----------|
| T-1 | Hierarchical tree of entities with geometric presence | Must |
| T-2 | Expand/collapse nodes | Must |
| T-3 | Entity kind icons (structure, solid, intermediate, port) | Must |
| T-4 | Type label per node | Should |
| T-5 | Eye icon per node cycling show/ghost/hidden | Must |
| T-6 | Default: show `geometry`, hide other solids (when `Physical`) | Must |
| T-7 | Click to select (synced with viewport + property editor) | Must |
| T-8 | Double-click to fly camera to entity | Should |
| T-9 | Right-click context menu (show/ghost/hide this, children, only) | Must |
| T-10 | Ctrl+click multi-select | Must |
| T-11 | Shift+click range select | Must |
| T-12 | Multi-select + bulk visibility change | Must |
| T-13 | Tabbed with PropertyEditor | Must |
| T-14 | Tree rebuilds on re-evaluation without losing selection | Must |

### 13.2 Visibility

| ID | Requirement | Priority |
|----|-------------|----------|
| VIS-1 | Three visibility states: show, ghost, hidden | Must |
| VIS-2 | Ghost rendering: transparent, non-selectable, neutral color | Must |
| VIS-3 | Hidden entities removed from scene entirely | Must |
| VIS-4 | Cascading: parent change propagates to all descendants | Must |
| VIS-5 | Override: child explicit state survives parent cascade | Must |
| VIS-6 | "Inherit" mode: child follows parent (default) | Must |
| VIS-7 | Keyboard shortcut for visibility toggle (H for hide, G for ghost) | Should |

### 13.3 View Management

| ID | Requirement | Priority |
|----|-------------|----------|
| VM-1 | Auto "Default" view with Physical-aware rules | Must |
| VM-2 | Auto "All geometry" view | Should |
| VM-3 | Purpose-linked auto views | Should |
| VM-4 | View selector dropdown at top of tree panel | Must |
| VM-5 | Create new named view | Must |
| VM-6 | Rename user views | Must |
| VM-7 | Delete user views | Must |
| VM-8 | Duplicate view | Should |
| VM-9 | Copy-on-write for auto views | Must |
| VM-10 | Visual distinction between auto and user views | Must |

### 13.4 Viewports

| ID | Requirement | Priority |
|----|-------------|----------|
| VP-1 | Def preview viewport (prototype of selected definition) | Must |
| VP-2 | Design viewport (instance tree with view controls) | Must |
| VP-3 | Adaptive layout: both/either/neither active | Must |
| VP-4 | Independent camera state per viewport | Must |
| VP-5 | Def preview updates on cursor move between definitions | Must |
| VP-6 | Viewport label ("Preview: BoltFlange" / "Design") | Must |
| VP-7 | Greyed-out minimized state for inactive viewports | Must |
| VP-8 | View-viewport decoupling (viewport references view by ID) | Must |

### 13.5 Persistence

| ID | Requirement | Priority |
|----|-------------|----------|
| P-1 | localStorage persistence of view state per file | Must |
| P-2 | Sidecar `.ri.views.json` for durable/shareable persistence | Should |
| P-3 | Entity path exact matching on re-evaluation | Must |
| P-4 | Stale path preservation (for undo/branch-switch restoration) | Must |
| P-5 | Fuzzy path recovery with user confirmation | Should |
| P-6 | Camera state persistence per viewport | Should |

### 13.6 Multi-Selection

| ID | Requirement | Priority |
|----|-------------|----------|
| MS-1 | Ctrl+click toggle in tree and viewport | Must |
| MS-2 | Shift+click range select in tree | Must |
| MS-3 | Multi-select visual feedback (wireframe on all selected) | Must |
| MS-4 | Bulk visibility change on multi-selection | Must |
| MS-5 | Selection sync between tree and viewport | Must |
| MS-6 | Escape to clear selection | Must |

---

## 14. Performance Requirements

| Metric | Target | Rationale |
|--------|--------|-----------|
| Tree rebuild on re-evaluation | < 50ms | Must not block the UI thread; tree is typically < 1000 nodes |
| Visibility toggle (show/ghost/hidden) | < 16ms | Must be instantaneous (within one frame) |
| View switch | < 100ms | Material swaps + scene add/remove for all affected meshes |
| Ghost rendering overhead | < 2ms per frame | Transparent pass with no lighting is cheap |
| Def preview generation | < 500ms | Same target as parameter edit latency |
| localStorage read/write | < 10ms | Small JSON payloads |
| Sidecar file write | < 50ms | Async, non-blocking |

---

## 15. Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| **Entity tree not available from engine** | Backend work needed to walk evaluation graph hierarchically | High (known gap) | `get_entity_tree` command is new work; design the tree node structure to be simple and flat-serializable |
| **Def preview latency** | Creating a temporary evaluation scope per cursor move may be too slow | Medium | Cache by definition content hash. Debounce cursor moves (200ms). Only re-evaluate when definition body actually changes. |
| **Ghost rendering z-fighting** | Transparent ghost meshes may z-fight with opaque meshes at shared boundaries | Medium | Polygon offset on ghost material. Or render ghost slightly scaled (1.001x) to avoid coplanar surfaces. |
| **View state bloat** | Complex assemblies with many entities produce large view state maps | Low | Only store explicit overrides, not computed effective states. Most entities will be "inherit." |
| **Sidecar file merge conflicts** | `.ri.views.json` in version control will conflict on multi-person edits | Medium | Accept this as a known limitation. Views are presentation, not design — conflicts are low-stakes. Merge strategy: last-write-wins per view name. |
| **Fuzzy path matching false positives** | Heuristic matching could re-bind visibility to the wrong entity | Low | Never auto-apply fuzzy matches. Always prompt user. Stale entries are safe (they just sit idle until the path returns). |
| **Dual viewport layout complexity** | Adaptive layout with minimize/maximize adds UI complexity | Medium | Start with a simple horizontal split. Minimize = collapse to 40px strip with label. No fancy animations. |

---

## 16. Resolved Decisions

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| 1 | Tree shows definition or instance structure? | **Instance structure**, with def preview viewport for definitions | Instances are what you compose and control; definitions are templates. Dual viewport covers both needs. |
| 2 | View persistence across re-evaluation? | **Entity path matching** with fuzzy recovery and stale preservation | Stable entity identity is too hard for v0.1. Path matching works well for common cases; stale preservation handles the rest. |
| 3 | Multi-viewport? | **Yes, designed in from the start** | Good abstraction boundary. Two viewports in v0.1 (def preview + design). Split viewports in a future version. |
| 4 | Do visibility changes cascade? | **Yes, recursively to all children** | A thing is composed of its members. Per-child override available when you need specificity. |
| 5 | Who creates views? | **Both.** Auto views (system) + named views (user), with copy-on-write | Auto views are always clean. User modifications are preserved via COW. |
| 6 | Can the language reference view state? | **No.** Presentation references design, never the reverse | CSS/HTML separation. Keeps the core language clean. Presentation is an overlay. |
| 7 | View state in `.ri` files? | **No.** Sidecar file + localStorage | Separation of concerns. Design source should not contain presentation state. |
| 8 | Ghost rendering approach | **MeshBasicMaterial, transparent, no depth-write** | Simple, performant, visually clear. No lighting = no visual competition with opaque geometry. |

---

## 17. Implementation Sequence

See task decomposition (separate). Rough ordering:

1. Design Tree panel MVP + basic show/ghost/hidden
2. Ghost rendering in Three.js
3. View state model with cascading and override
4. Multi-selection (tree + viewport)
5. Auto view generation (Default + All Geometry)
6. Named views + copy-on-write
7. Viewport-view decoupling + viewport store
8. Dual viewport (def preview + design)
9. View persistence (localStorage + sidecar + fuzzy matching)
10. Engine: entity tree emission + def preview + identity hints
