# PRD: Support realization policy — one module decides which DOFs a declared support constrains, for every solver

**Status:** active. Authored on 2026-10-18/19 by the gap-fill stream session for **support-realization-policy**
(`plans/gap-fill-deliberation-reify-20261016.md` §K5-2, with §Uncontested block → K5-3 and → K5-4).
Decomposed in the same session (2026-10-19): α₁ #9379, α₂ #9380, β #9381, γ #9382, δ #9383, ε #9384 (high, gate
#9171), Ω #9385; out-of-plan decision milestone #9386 (rigid-coupling hinge; ← Ω, #8612). Shape: **B+H** (contract §6, boundary-test sketch §7). FEA and
ComputeNode dispatch are overlay-listed load-bearing seams. Milestone: v0_6.

**Code anchors** were verified against main `eb0a855dad` (2026-10-18). `git diff 04be9302ea..main` over
`crates/reify-eval/src/modal_ops.rs`, `compute_targets/{buckling,shell_solve}.rs` and
`crates/reify-solver-elastic/src/shell_boundary.rs` changes no realization anchor:

- `elastic_static.rs` has three comment-only hunks (the `#4092` → `#5313` owner rewording);
- reify-solver-elastic has S14 #8616's shell-frame move (import paths only).

So the hotspot survey's anchors (taken at `0e60fa138d`) still hold. Main moves fast: cite by symbol, and
re-locate lines at implementation time.

**Finding keys owned (3), all hotspot-survey-reify-20261015:**

- fk-47257bcb2c8c (high, `analysis-trampolines.1`);
- fk-ec25b28cba70 (`analysis-trampolines.3`);
- fk-5edeea9c474a (`analysis-trampolines.4`).

Their statements and proposals are in `docs/notes/bug-hotspot-survey-2026-10-15-full-findings.json`. §10 maps
each key to its leaf.

---

## §1 Goal

The same declaration, `[PinnedSupport("x_min"), FixedSupport("x_max")]`, gives four different boundary
problems on main today:

| solver | what it builds |
|---|---|
| modal | a transverse pin, decided by whether another face is named |
| elastic body path | a full clamp of the AABB x-min face; `x_max` is ignored |
| elastic dims | the root clamp; after S16 μ #9119, a refusal |
| buckling | pin-pin, whatever the list says |

Which DOFs of which nodes a declared support constrains is decided at five independent sites. Each site has
its own target vocabulary, its own kind semantics and its own refusal policy (fk-47257bcb2c8c).

After this PRD:

- **One module decides support realization for every solver.** It is
  `crates/reify-solver-elastic/src/supports/` and holds:
  - a pure policy table `constraint_pattern(kind, body, frame)`, which extends and absorbs
    `shell_boundary::build_support_bcs`;
  - a target resolver `resolve_support_target(&SupportTarget, &BoundaryView)`;
  - a composer `realize_supports(&[SupportDecl], &BoundaryView, &SupportCapability)`. It applies the one
    model-level anchor rule and returns `Realization { bcs, applied, … }` or one coded refusal per support it
    cannot realize;
  - every solver's `SupportCapability` row, as constants in one list.
- **`PinnedSupport` has one meaning: a hinge** (ruling R3).
  - On an axis-aligned box face, the two tangential translations are fixed on every face node, and the normal
    translation is free.
  - On a shell node, the translations are fixed and the rotations are free.
  - On a face whose tangent frame the solver does not know (a realized body face), a hinge is not realizable,
    so the support is refused (R9).
  - Modal's context-dependent `FaceCompany` reinterpretation and buckling's corner anchor both die.
- **Each solver path names one `SupportCapability` row.** The row states which kinds, target forms and faces it
  realizes, whether it adds an implicit root clamp, and what it does with a mechanism.
  - Every declared support is either in `Realization.applied` or is one coded `FeaSupportNotRealized` Error
    naming the support and its span. The entry then returns `ComputeOutcome::Failed`.
  - A new solver or support kind is one capability row plus one table row, not another realization site.
- **The applied echo is typed.** `Applied<T>` can only be built from the realizer's own indices, so a result's
  `boundary_conditions` lists exactly the supports that were realized (K5-4, fk-5edeea9c474a).
- **INV-SF-3 `declared-intent-consumed-or-diagnosed` covers supports.** One test drives every registered row
  against the full kind × target-form × view matrix. A second test fails when a support-taking solver names no
  row.

Consumers that become calls instead of ports:

- buckling's support honouring (#7081);
- modal's `realize_supports` (S16 ξ #9120), which becomes an adapter;
- μ's dims refusal (#9119), which becomes the elastic dims capability row;
- the coordinate-target point supports (#7189, #8251, #8256) and the selector-typed modal supports (#7143). They
  arrive as target forms through the same call.

## §2 Background: what the code does at `eb0a855dad`

### §2.1 The five realization sites (fk-47257bcb2c8c, re-verified)

| # | site | vocabulary | kind semantics | unrealizable support |
|---|---|---|---|---|
| 1 | `modal_ops.rs::build_dirichlet_bcs` (`support_targets`, `face_bound`, `face_company`, `face_realization`, `per_face_bcs`, `pin_pin_axial_anchor`) | `x_min`…`z_max` (`face_bound`; `is_beam_axis_end_face` is a second copy) | Fixed → 3-DOF clamp. Pinned on an x end face *with another distinct face named* → `PinTransverse`: u_z on every face node plus u_y at the node nearest `[x, 0, h/2]`. Otherwise Pinned → clamp (a lone pin, duplicates, a non-end face). The pin-pin case adds one u_x at the x_min anchor node. The beam axis is hard-wired (`assert_eq!(axis, 0)`). | silent `continue`, so `FixedSupport("root")` is a free-free solve; an unknown type_name falls back to Fixed |
| 2 | `elastic_static.rs` dims path (`SYNTHETIC_CLAMP_FACE = "root"`, `any_support_targets`) | `"root"` only, for a Warning | the synthetic root face is clamped whatever the list says | `FeaUnderConstrained` Warning; μ #9119 makes it a coded Error |
| 3 | `elastic_static.rs` realized path (`loads_supports_to_bc_node_sets`, `target_node_set`, `realized_cantilever_bc_node_sets`) | a `Value::List<GeometryHandle>` target → `bc_resolve::boundary_node_set`; any String target or an empty list → `None` → the AABB x-min face by coordinate | kind-blind: all 3 DOFs | an empty handle resolution → `FeaSelectorNoMatch` (Failed); a non-root String → silently the AABB x-min clamp |
| 4 | `buckling.rs::solve_buckling_trampoline` | none: `let _ = &value_inputs[5];` | P1 and P2: u_x, u_y on every node of both z-end faces, plus u_z at node (0,0,0) | the list is discarded (#7081 is the owner) |
| 5 | reify-solver-elastic `shell_solve.rs` → `shell_boundary::build_support_bcs(&root_nodes, Fixed, Shell)` | none (the root edge) | the solver crate's (kind × body) table. `Pinned` is never constructed in production; `SupportCompatibility` is bound to `_compat` and read by nothing. | n/a (μ refuses upstream) |

A sixth site is in flight on the unlanded branch `task/7189`: `point_support_bcs`, `dirichlet_nodes`, and
`compute_targets/point_support.rs`. It realizes point patches with all three translations, and adds a third
static "implicit clamp" policy (clamp iff the list has no PointSupport or any face-class support).

### §2.2 Premises the code contradicts or narrows (G3; the code wins)

- **P1 — layering.** reify-solver-elastic depends on reify-ir, reify-kernel-gmsh, faer, inventory and tracing.
  It cannot name `SupportSpec`: S15 γ #8683 puts it in reify-eval's `compute_targets/stdlib_inputs/`, and S5 κ₁
  #8999 moves it to reify-analysis. Both are above the solver crate. It cannot name `PointTarget`
  (`pub(crate)` in reify-eval `node_patch.rs`, CT α #8251) either.
  - So the ruled `resolve_support_target(&SupportSpec, …)` takes a solver-side `SupportDecl`/`SupportTarget`
    (§6.2).
  - One adapter beside the decoder maps `SupportSpec → SupportDecl`.
  - Ruling R4 adds a reify-core dependency so that the module can build coded Diagnostics itself.
- **P2 — points are a target form, not a view.** A point resolves by coordinate against whatever mesh is being
  solved (CT C4 I1), which may be the synthetic box, the promoted modal mesh or a realized body. Ruling R2 makes
  them `SupportTarget::Patch`, resolvable on both views. There is no `BoundaryView::Points`.
- **P3 — the realized path has two behaviours, not one clamp.**
  - Only a `Value::List<GeometryHandle>` target reaches the union-of-faces clamp. No production producer
    exists until #5312/#5313.
  - Every committed body fixture uses a String target, which falls to `realized_cantilever_bc_node_sets`: the
    AABB x-min face by coordinate.
  - That clamp also applies to an empty list, with the `FeaUnderConstrained` Warning.
  - A String target other than `root`/`x_min` is silently ignored today. Ruling R10 refuses it.
- **P4 — the two "pins" are different idealizations, and two of their choices are defects.**
  - Modal holds one transverse DOF face-wide and one lateral node; buckling holds both transverse DOFs
    face-wide.
  - Modal's axial anchor sits at the y=0 edge. Buckling's sits at a corner, which puts the whole axial reaction
    at one node off the centroid axis.
  - Modal realizes a lone pin as a clamp (`FaceCompany`), so a pin's meaning depends on what else is declared.
  - Ruling R3 settles this with one meaning (§5 D1).
- **P5 — buckling's examples target `"base"`.** That name is not a box face: `BoxFace::parse` takes
  `x_min`…`z_max` plus the `"root"` alias. When #7081 honours supports through the module,
  `FixedSupport(target: "base")` is a refusal, not "fixed-free". #7081 renames the targets (`z_min`).
- **P6 — the dead-site ratchet rows cannot live in the solver host.**
  `crates/reify-solver-elastic/tests/adoption_ratchets.rs` scans only reify-solver-elastic. Rows counting the
  deleted reify-eval/reify-analysis sites go on S5 α #8984's `tests/infra/test_analysis_stratum_ratchet.py`.
- **P7 — the body overload is not observable through `reify eval`.** The CLI registers no gmsh kernel
  ("no gmsh kernel registered … leaving the BRep/Mesh fallback"; #6660 owns the fix).
  - The body overload's pre-hydration guard (`realized_solver_mesh_with_handle(..).is_none()` →
    `ComputeOutcome::Failed { diagnostics: vec![] }`) therefore returns before any support is read. `reify
    eval` exits 0 with the hollow default `ElasticResult`.
  - This was probed 2026-10-18/19 on the main-equivalent debug binary. The stale 2026-09-01 release binary still
    "solves on the fallback", so CLI evidence depends on which binary is used.
  - A body-overload signal is therefore an in-process test in `crates/reify-eval/tests`, where gmsh is a
    dev-dependency. That crate's existing body e2e binary (`solve_elastic_static_body_e2e.rs`) is the pattern;
    no other reify-eval test binary may reference gmsh symbols. `reify-eval-fea-tests` has no gmsh.
- **P8 — ξ #9120 already delivers modal totality and the applied echo modal-locally** (amended 2026-10-17).
  The finding line "ξ keeps the skip" predates that amendment. This stream makes both cross-solver and typed.
- **P9 — cross-crate registration cannot work from the solver crate.** `inventory` collects only `submit!`s
  linked into the binary under test, and reify-solver-elastic's test binaries cannot link reify-eval or
  reify-analysis (both depend on it). So the capability rows live in the module, one constant list (§5 D12).

### §2.3 The two G3 items the brief left open

- **"The realized-path all-DOF clamp on tets is documented-correct" (S15 γ's skeptic note).** Re-verified:
  `loads_supports_to_bc_node_sets`' doc argues that `(Tet, Pinned) => (3, 3, PinnedOnTetEquivalentToFixed)` makes
  a face-pin equal a face-clamp.
  - That is true at the DOF level: fixing every translation of a face fixes its rotation.
  - It is not the meaning a `PinnedSupport` author intends (a hinge).
  - Ruling R9 therefore refuses Pinned on realized faces. `FixedSupport` keeps today's clamp, with no number
    change.
- **#7143 and #7189 read in full.**
  - #7143 consumes #5313's selector → node-set seam on #7142's body-arg modal overload and rejects a non-selector
    target. In this module that is `SupportTarget::Faces` on `BoundaryView::Realized`.
  - #7189 has no Fixed/Pinned split for points: every patch node gets three translations, which is exactly the
    hinge semantics at a point. It adds the "implicit clamp unless point-only" rule and a static rigid-body
    guard. Both become module rules (`ImplicitClamp`, `free_rigid_modes`) in δ.

## §3 Pre-conditions and already-filed owners

- **S15 γ #8683** (in #8612's closure): `SupportSpec { kind: reify_solver_elastic::SupportKind, target }`,
  decoded once, with an unknown type_name → `UnknownVariant`. It is the input type, consumed unchanged until δ.
  α₂ and #7081 depend on it; β reaches it through μ. A `PointSupport` decodes to `UnknownVariant` there, so the
  point form of the decode is δ's: δ extends the landed decoder (a follow-on edit, no write to #8683) unless
  #7189's rebase or #8256 has already added it.
- **S16 ο #9121** (→ ξ #9120 → ν #9091): `BoxFace` (`parse`, with `"root"` → `XMin` for every box-mesh consumer;
  `axis`; `is_max`) and `StructuredBoxMesh`. The module sits beside them and α₁ depends on ο.
- **S16 ξ #9120:** modal-local `realize_supports` with K5-3 totality and the K5-4 applied echo. α₂ absorbs it as
  a call. Ruling R1 asks ξ to mint `FeaSupportNotRealized` if it lands before μ.
- **S16 μ #9119:** the dims-support refusal. It is renamed to `FeaSupportNotRealized` by ruling R1 (three-write
  correction at filing, §11). β depends on it.
- **S15 ζ₀ #9287** (in the closure; the named G1 exception, before ζ #8693): modal error paths return
  `ComputeOutcome::Failed`, and `ComputeOutcome::completed` debug-asserts no Error. Every refusal here leaves the
  entry as `Failed`, never as `completed(…)` with an Error. #9287 is upstream of α₁ through ο → ξ → ν → ζ.
- **S5 α #8984** (in the closure): `tests/infra/test_analysis_stratum_ratchet.py`, the host for the dead-site
  rows. It is upstream through ο.
- **Placement in reify-analysis.**
  - The modal family's order is fixed: ο #9121 → ξ #9120 → ν #9091 → S5 μ #9003 → κ₁ #8999. So α₂ always lands
    after `modal_ops` has moved into reify-analysis's modal family, and locates it by grep.
  - Only the elastic move (S5 ν #9004, in the closure) is "whichever lands second adapts", and that applies to
    β and γ.
- **#7189 and CT α #8251:** the point-support resolver. δ depends on both.

## §4 Rulings

### From the record (binding; verbatim headlines, full text in `plans/gap-fill-deliberation-reify-20261016.md`)

- **K5-2** (human, 2026-10-16): A. New stream `support-realization-policy`: one module in reify-solver-elastic
  decides which DOFs a declared support constrains for every solver — `constraint_pattern(kind, body,
  Idealization { Continuum | BeamEnd { axis } })` extending `shell_boundary::build_support_bcs`, and
  `resolve_support_target(&SupportSpec, &BoundaryView { StructuredBox | Realized | Points }) ->
  Result<NodeSet, SupportRefusal>`; each trampoline declares a `SupportCapability`; every unrealizable declared
  support is one coded refusal.
  - Binding constraints: the elastic dims capability stays "root clamp only"; R3's
    `FeaDimsSupportNotRealized` behaviour is kept (only its scope or name may change); the realized elastic
    path's capability row is today's all-DOF clamp; SolveBoundary and nodal_basis stay coordinate-target-fea's,
    consumed through `BoundaryView::Points`.
  - Follow-on per G1: after S15 γ #8683 and S16 ο #9121. K5-3's modal totality and K5-4's applied-echo rule
    fold into its refusal and realization types.
- **K5-3** (uncontested block): modal support decode and realization total. #9120's `realize_supports` emits
  one coded refusal per unrealizable support (entry Failed); `BoxFace::parse("root")` → XMin for every
  box-mesh consumer.
- **K5-4** (uncontested block): ModalResult echo fields come from what was applied
  (`DirichletRealization.applied`, the damping plans' applied descriptor).
- **G1** (human, 2026-10-16): #8612's transitive dependency closure is protected. No leaf here becomes a
  prerequisite of a closure task. The closure was recomputed live on 2026-10-18 (240 non-terminal tasks) and
  again on the filing day, 2026-10-19 (239).
  - In the closure: #8683, #8984, #8999, #9004, #9118, #8693, #9287, #8248.
  - Not in the closure: #9119, #9120, #9121, #9091, #7081, #7189, #8251, #8253, #8256, #7142, #7143, #8247,
    #5312, #7871, #7213, #9171.

### This session (Leo, 2026-10-18/19; answers verbatim)

- **R1 — refusal code.** "One FeaSupportNotRealized (Recommended)".
  - Whichever of ξ #9120 / μ #9119 lands first mints `FeaSupportNotRealized`, and the other reuses it.
  - μ's `dims-support-code` delivered check is corrected at filing in three writes: S16 sidecar `.yaml`, its
    `.md` twin and #9119's `metadata.delivered_checks`.
  - This replaces the brief's `FeaDimsSupportNotRealized`. R3's behaviour is unchanged.
- **R2 — BoundaryView shape.** "2 views + Patch target (Recommended)".
  - `BoundaryView { StructuredBox, Realized }`.
  - Points are `SupportTarget::Patch`.
- **R3 — what a pin means.** Leo's first answer was to discuss further: "I have a hunch that the discrepancy is
  because at least one of the pin definitions is _wrong_. I'm comfortable changing the definition if it's
  justified. Is there a clearly correct single meaning for pin? … Failing that, is there a common single
  definition we could settle on that will work well everywhere?" After the analysis in §5 D1: "B hinge face,
  measure in-leaf, milestone for D later".
- **R4 — layering.** "Add reify-core". reify-solver-elastic takes a reify-core dependency. Codes are minted in
  reify-core and the module builds coded Diagnostics with span labels itself. The crate's `diagnostics.rs`
  header rule ("NO reify-core imports allowed") is amended in α₁; the existing `FeaFailure` mapping stays as it
  is.
- **R5 — the two medium keys.** "α takes both as x_finding_key (Recommended)". After the critic's split of α
  (§9), the modal leaf α₂ carries them, because it closes modal totality and the typed echo.
- **R6 — the high key.** "On ε, after α β γ only (Recommended)". #9171 ← ε, so the survey gate never waits on
  the coordinate-target chain.
- **R7 — coordinate-target ordering.** "δ waits; #8256/#7143 adapt second (Recommended)".
- **R8 — the rigid-coupling milestone (option D).** "After #8612 too". It is an out-of-plan decision milestone
  that depends on Ω and on #8612.
- **R9 — PinnedSupport on a realized face.** "Refuse (Error)". This is Leo's explicit override of K5-2's
  "realized row = today's all-DOF clamp" for `Pinned` only; `FixedSupport` on the realized path is unchanged,
  and so is the empty-list clamp (D13).
- **R10 — a non-root Named target on the body overload.** "Refuse, FeaSupportNotRealized (Recommended)".
- **R11 — a lone hinge (a mechanism).** "Modal warns, others refuse (Recommended)".
- **R12 — INV-SF-3 lockstep.** "design-invariants + census + pointer (Recommended)":
  - an in-place amendment of `docs/legibility/design-invariants.md` §INV-SF-3, heading byte-identical;
  - one census row;
  - the program-doc pointer, which is the deliberation lead's;
  - no `docs/invariants.md` row and no CLAUDE.md edit.
- **R13 — split α** (put after the critic pass, 2026-10-19). "Split; keys on α₂ (Recommended)".
  - α₁ (the module, byte-identical) and α₂ (modal adoption and the hinge).
  - β, γ ← α₁; δ, ε ← α₂.
  - fk-ec25b28cba70 and fk-5edeea9c474a are α₂'s `x_finding_key`.
- **R14 — mechanism codes** (2026-10-19). "One general code; δ folds the point one (Recommended)".
  - α₁ mints `FeaSupportsUnderRestrained` (Error), and α₂ mints its Warning sibling for modal.
  - δ folds CT's `FeaPointSupportsUnderRestrained` into the general code and hand-repoints #8251's
    done-producer check (D4).

## §5 Resolved design decisions

- **D1 — one meaning of `PinnedSupport`: a hinge (R3).**
  - **The meaning.** A pinned support restrains translation and leaves rotation free. FE codes state it at the
    DOF level: translations fixed, rotations free.
    - That is exact on nodes that carry rotations (shells).
    - It degenerates on a solid face. Fixing every translation of a face fixes its rotation, so a "pinned face"
      on tets is a clamp; Abaqus documents PINNED ≡ ENCASTRE on solids.
    - Every Dirichlet-only realization of a hinge on a solid face is therefore an idealization.
  - **The hinge face (B).** On an axis-aligned box face, the two tangential translations are fixed on every
    face node and the normal translation is free, so the face rotates freely about every in-plane axis.
    Fixing the tangential translations also restrains torsion about the face normal.
  - **The anchor rule.** The normal translation is restrained by one model-level rule (D3).
  - **Why this one.** Of the Dirichlet-only realizations it alone:
    - treats both transverse directions alike;
    - puts the axial reaction on the centroid axis;
    - does not depend on what else is declared.
  - **What changes, measured inside the leaves (α₂, γ) and never assumed:**
    - Modal: the second transverse direction becomes face-wide, and the P2 axial anchor moves to the face
      centroid. On P1, `build_beam_mesh`'s `ny = 1` makes the node nearest the analytic centre tie to today's
      y=0 edge node; the tie goes to the lowest index under `min_by(total_cmp)`.
    - These modal configurations change (D11): two pins on the same face (duplicates, today a clamp) and a lone
      Pinned on a non-end face (today a clamp) both become hinges.
    - Buckling: the axial anchor moves from the corner (0,0,0) to the z_min centroid. Measured before filing
      (2026-10-19, release): the calibrated pin-pin error goes from 9.17% to 9.21% against a 10% bound
      (`euler_column_pin_pin.rs`, nx = ny = 8, nz = 160).
  - **What dies:** `FaceCompany`, `PinTransverse`, `I_ModalPinnedFaceRealization`.
  - **A lone hinge is a mechanism and is reported (R11, D4),** not silently clamped.
  - **Rejected alternatives:**
    - (C) DOF-level only, which makes simply supported beams inexpressible (modal f1 × 2.27, buckling
      P_cr × 4);
    - (B′) keeping two pin rows;
    - keeping today's rows.
  - **The exact hinge is future work.** A distributing (RBE3-style) coupling of the face to a pinned centroid
    reference node is option D, a decision milestone (R8; §9 M).
- **D2 — the third table parameter is the support's frame, not a beam idealization.**
  - Under D1 a pin's DOF pattern depends on the face's normal, not on a beam axis. So the ruled `Idealization {
    Continuum | BeamEnd { axis } }` becomes `SupportFrame { BoxFace { normal: Axis } | Unframed }`, and the
    beam-axis parameter disappears.
  - `Unframed` covers realized faces and point patches.
  - A point patch is a hinge by construction (three translations at a point, rotations free). A realized face
    cannot express one without per-node tangent restraint (#8256's `NodalBasis`), so Pinned on it is refused
    (R9).
  - This is a consequence of R3, recorded here rather than re-asked.
- **D3 — one model-level anchor rule, stated once (in `realize_supports`), in rank terms.** It removes the
  translation rigid modes that hinges leave open, and nothing else:
  - after every declared support and any implicit clamp is emitted, take each `Pinned` box-face decl in
    declaration order;
  - if rigid translation along that face's normal axis is not in the span of the constrained directions, add
    one homogeneous BC along that axis at the node nearest the face's analytic centre. The centre is computed
    from `extent`: `0` or `extent[a]` on the normal axis, `extent/2` on the other two — never a mean of
    coordinates. Ties go to the lowest index, via `nearest_node`.
  - The span test is the same helper that computes `free_rigid_modes` (D4). Oblique `restrain` directions (δ)
    therefore need no second rule.
  - It reproduces pin-pin (one end anchored) and does nothing when a Fixed face or an implicit clamp already
    restrains the axis.
  - It never adds rotational restraint, so a lone hinge stays a mechanism with exactly two free rotations.
- **D4 — mechanisms are counted once, and every policy reports from that count.**
  - `Realization.free_rigid_modes` is computed in the module as 6 − rank of the rigid-body modes restricted to
    the constrained DOFs (rotated directions for basis-changed nodes). It is the general form of 7189's
    `unrestrained_rigid_body_modes` and of #8256's rank rule over directions, and one home for both.
  - Each capability declares `MechanismPolicy::{Warn, Refuse}`:
    - **Refuse** (static, buckling, shell): one `FeaSupportsUnderRestrained` Error naming the free-mode count.
      α₁ mints the code (R14).
    - **Warn** (modal, R11): one coded Warning at realization time, naming the hinge decl(s) and the count. It
      is the Warning-severity sibling of `FeaSupportsUnderRestrained`, with its spelling chosen in α₂ (S16's
      "one rejection reason, one code per severity" rule). The modal solve then proceeds.
      - Below the 1024-DOF dense ceiling, the eigensolver also reports `W_ModalRigidBodyMode` for the free
        modes.
      - Above the ceiling, the solve fails with `E_ModalNoModesComputed` (Failed after #9287). The realization
        Warning is what names the cause, in both regimes.
  - **The point-only code folds in.** CT's `FeaPointSupportsUnderRestrained` will have shipped by δ's time
    (#7189's rebase mints it, and #8251's done-producer check `^    FeaPointSupportsUnderRestrained\b` greps
    it). δ folds it into `FeaSupportsUnderRestrained`, because it is one rejection reason, so one code; the
    point name would lie for a face hinge (heuristic 1).
    - δ hand-repoints #8251's done-producer check (coordinate-target sidecar `.yaml`, its `.md` twin, #8251's
      `metadata.delivered_checks`) and any #8256 assertion by identity, in the same diff (S5 C7 step 4).
    - This is a cross-reference line to the coordinate-target owner.
- **D5 — refusals.** The module builds every refusal's Diagnostic itself (R4):
  - code `FeaSupportNotRealized` (R1) for a kind, target form or face outside the capability, or an
    unparseable name;
  - `FeaSelectorNoMatch` (existing, byte-stable message) for a `Faces` target that resolves to no node;
  - `FeaSupportsUnderRestrained` (D4).
  - Every `SupportRefusal` variant carries the decl's `SupportLabel { type_name, index, span }` and its target
    as authored. So the one mapping site, `SupportRefusal::diagnostic(&self, cap)`, can name the support and
    give that instance's own span label (today's `first_instance_source_span` gives the first instance's span
    only).
  - It carries the overload-specific remedy from the capability row: μ's "use the body overload" for dims, and
    "only the root clamp can be modelled" for Field.
  - `realize_supports` returns every refusal (one per support), and never a partial realization.
  - The caller returns `ComputeOutcome::Failed` carrying them (INV-SF-2; ζ₀'s rule).
- **D6 — the adapter is the only place that knows `Value` shapes (heuristic 12).** One function beside the
  `SupportSpec` decoder (S15's `stdlib_inputs`, the shared foundation, so both the modal and FEA families may
  use it), `support_decls(&[SupportSpec]) -> Vec<SupportDecl>`, is index-preserving. It maps:
  - a String (and, after #5312, a `Selector` Named leaf) to `Named`;
  - resolved handles to `Faces`;
  - after δ, a `PointSupport` resolved through CT α's `patch_nodes` on the mesh being solved (CT I1: every call,
    every adaptive iteration) to `Patch`. An off-body point is formatted by the caller as CT's `FeaPointOffBody`
    before the module sees it.

  `Named` is never parsed in the adapter: `BoxFace::parse` (ο's, with the `"root"` alias) stays the single
  parser, called inside `resolve_support_target` (heuristic 11). A support built in code, such as γ's
  synthesized pin-pin, uses `SupportTarget::Face(BoxFace)` and never renders a name to be parsed back. Whichever
  of α₂ and β lands first adds `support_decls`; the other reuses it.
- **D7 — `Applied<T>` is the K5-4 brand.**
  - Its field is private. The only constructors are inside `supports::realize`, plus
    `Applied<Vec<usize>>::select(&self, decls: &[T]) -> Applied<Vec<T>>`, which picks the decls at the
    realizer's indices (an out-of-range index panics).
  - There is no arbitrary `map`: a closure could replace the contents and echo everything (heuristic 10).
  - The modal adapter builds `Applied<Vec<SupportSpec>>` with `select`, which feeds ζ #8693's
    `ModalResultParts.boundary_conditions`.
- **D8 — the elastic dims row keeps R1/R3 exactly.**
  - `ELASTIC_DIMS` admits only `Fixed` on `XMin`, with `implicit_clamp: Some(XMin)`: the synthetic root is
    clamped whatever the list says.
  - Every other decl is a refusal.
  - The dims root clamp's node set and DOFs are those of today's `solve_cantilever_fea` root loop.
  - The coordinate kinds S16 C2 invariant 3 excludes arrive as `Patch` after δ, and the clamp's `unless:
    PointOnly` mirrors 7189's rule and CT's `apply_face_clamp`.
- **D9 — the solver crate gains reify-core (R4).** This is the first such edge. reify-ir already depends on
  reify-core, so no cycle forms. The S5 BT1/BT2 closures are unaffected: reify-analysis already depends on both.
  α₁ amends the `diagnostics.rs` header rule and records the edge in its commit.
- **D10 — single homes.**
  - `nearest_node` moves from `modal_ops.rs` into the module, with its non-finite tests. CT α #8251 imports it
    from there; whichever lands second adapts.
  - "The nodes of a box face" has one home: the module's coordinate predicate. That predicate is needed
    because a P2-promoted node array is not a `StructuredBoxMesh`. α₁ makes ο's `StructuredBoxMesh::nodes_on`
    delegate to it, or deletes `nodes_on` if nothing calls it (a cross-reference line to ο).
  - The kernel-less face → node map (`bc_resolve::boundary_node_set`) must be nameable from the solver crate
    (§13 OQ-1).
  - `shell_boundary.rs` is absorbed into `supports/policy.rs`; `lib.rs` re-exports the moved names at the crate
    root.
  - `SupportCompatibility` dies. Its one fact (Pinned on a tet face is not a hinge) is now the policy table's
    refusal.
- **D11 — numbers.**
  - Unchanged by construction:
    - every `FixedSupport` realization (modal clamp, dims root clamp, realized AABB clamp including the
      empty-list case, `Faces` union, shell root);
    - the order of homogeneous BCs (row elimination is exact for zero values).
  - Moved by D1, and measured in-leaf against the existing bands (never a widened band):
    - modal configurations with `PinnedSupport` (`simply_supported_beam_modes`, `printer_gantry_modes`,
      `printer_print_envelope`, `clamped_clamped_beam_modes.ri`'s pin-pin and propped solves,
      `prj/printer_v01/printer.ri`'s `opts_ss` / `opts_field`, and the unit-test configurations with duplicate
      pins or a lone non-end pin);
    - buckling pin-pin.
  - A band crossed is an escalation, not a re-baseline.
- **D12 — the capability rows live in the module (P9; heuristic 11).**
  - Each row is a `pub const` in `supports/capability.rs`, and `pub const REGISTERED: &[&SupportCapability]`
    lists them all. A trampoline names its row; it never declares one.
  - The leaf that migrates a solver adds that solver's row: α₁ `ELASTIC_SHELL`, α₂ `MODAL_BOX`, β
    `ELASTIC_DIMS`/`ELASTIC_FIELD`/`ELASTIC_BODY`, γ `BUCKLING_COLUMN`.
  - The solver-crate matrix test iterates `REGISTERED`.
  - A reify-eval/analysis-side test asserts that every production target whose stdlib declaration takes a
    `List<Support>` names a registered row by its `solver` id. This catches a new support-taking solver with no
    row. It lands in ε, which depends on every leaf that adds a production row (α₂, β, γ). Landed earlier, it
    would be red until `BUCKLING_COLUMN` exists, because `solve_buckling` takes `supports : List<Support>`.
- **D13 — the body row keeps today's implicit clamp, and only when nothing was declared.**
  - On main a handle-list target REPLACES the coordinate clamp (`solve_cantilever_fea`: `root_nodes = clamp`
    from `bc_override`), while a String target falls through to it.
  - So `ELASTIC_BODY` has `implicit_clamp: Some(ImplicitClamp { face: XMin, unless: AnyApplied })`: the AABB
    x-min clamp is emitted only when no declared support was applied.
  - On `BoundaryView::Realized`, `Named("root")` / `Face(XMin)` resolve to the same AABB set. A `Faces` union
    stands alone, as today. An empty list still solves with the AABB clamp and the `FeaUnderConstrained`
    Warning (`applied.is_empty() && implicit_clamp_applied`).
  - After δ, a point-only list is covered by the same rule (a `Patch` in `applied` suppresses the clamp), so
    the body row needs no `PointOnly` flip.
  - K5-2's "no change on the realized path" holds except where R9/R10 rule otherwise.
  - `ELASTIC_DIMS` keeps `unless: Never` (R1: the dims root is clamped whatever the list says), which becomes
    `PointOnly` after δ.

## §6 Contract (B+H)

### §6.1 Layout

```
crates/reify-solver-elastic/src/supports/
    mod.rs          the public surface; the one place the policy is stated
    policy.rs       SupportKind, SupportBodyKind, Axis, SupportFrame, DofMask, constraint_pattern
    target.rs       SupportDecl, SupportLabel, SupportTarget, BoundaryView, NodeSet, ResolvedTarget, resolve_support_target
    capability.rs   SupportCapability, KindSet, TargetForms, FaceSet, ImplicitClamp, MechanismPolicy; every row; REGISTERED
    realize.rs      Realization, Applied<T>, SupportRefusal, realize_supports, nearest_node, rigid-mode rank helper
```

Imports are strictly downward: `realize` → `capability`, `target`, `policy`; `target` → `policy` and ο's `BoxFace`.
Nothing imports `realize` (INV-ARCH-1; heuristic 13).

### §6.2 Signatures

```rust
// policy.rs
pub enum SupportKind { Fixed, Pinned }                       // moved from shell_boundary, unchanged
pub enum SupportBodyKind { Shell, Tet }                      // moved, unchanged
pub enum Axis { X, Y, Z }
pub enum SupportFrame { BoxFace { normal: Axis }, Unframed }
pub struct DofMask(/* bits over 0..stride */);
pub fn constraint_pattern(kind: SupportKind, body: SupportBodyKind, frame: SupportFrame)
    -> Result<DofMask, PatternRefusal>;

// target.rs
pub struct SupportLabel { pub type_name: String, pub index: usize, pub span: Option<SourceSpan> }
pub struct SupportDecl { pub kind: SupportKind, pub target: SupportTarget, pub label: SupportLabel }
pub enum SupportTarget {
    Named(String),                                            // as authored; BoxFace::parse at resolve time
    Face(BoxFace),                                            // built in code (γ's synthesized decls; parse-at-boundary results)
    Faces(Vec<GeometryHandleId>),                             // β: build-time resolved B-rep faces (#5312/#5313)
    Patch { nodes: Vec<usize>, restrain: Option<Vec<[f64; 3]>> },   // δ: pre-resolved on the mesh being solved
}
pub enum BoundaryView<'a> {
    StructuredBox { coords: &'a [[f64; 3]], extent: [f64; 3] },
    Realized { coords: &'a [[f64; 3]], boundary: Option<&'a BoundaryAssociation> },
}
pub struct NodeSet(/* sorted, deduplicated, non-empty */);
pub struct ResolvedTarget { pub nodes: NodeSet, pub frame: SupportFrame, pub face: Option<BoxFace> }
pub fn resolve_support_target(t: &SupportTarget, view: &BoundaryView) -> Result<ResolvedTarget, SupportRefusal>;

// capability.rs
pub struct SupportCapability {
    pub solver: &'static str,                  // the production target id, e.g. "solver::elastic_static"
    pub body: SupportBodyKind,
    pub kinds: KindSet,
    pub targets: TargetForms,                  // NAMED | FACE | FACES | PATCH
    pub faces: FaceSet,                        // Any | Only(&'static [BoxFace])
    pub implicit_clamp: Option<ImplicitClamp>, // { face: BoxFace, unless: Never | PointOnly | AnyApplied }
    pub mechanism: MechanismPolicy,            // Warn | Refuse
    pub remedy: &'static str,
}
impl SupportCapability { pub const fn checked(self) -> Self; }   // const-panics on an incoherent row
pub const REGISTERED: &[&SupportCapability] = &[ /* every row */ ];

// realize.rs
pub struct Applied<T>(/* private */);
impl<T> Applied<T> { pub fn get(&self) -> &T; }
impl Applied<Vec<usize>> { pub fn select<T: Clone>(&self, decls: &[T]) -> Applied<Vec<T>>; }
pub struct Realization {
    pub bcs: Vec<DirichletBc>,                 // sorted by dof, deduplicated, homogeneous
    pub bases: Vec<NodalBasis>,                // added by whichever of δ / CT ε #8256 lands second (NodalBasis is #8256's)
    pub applied: Applied<Vec<usize>>,          // decl indices, input order
    pub implicit_clamp_applied: bool,
    pub free_rigid_modes: u8,                  // 0..=6
    pub diagnostics: Vec<Diagnostic>,          // coded Warnings (MechanismPolicy::Warn)
}
pub enum SupportRefusal { /* NotAFace, TargetNotRealizable, KindNotRealizable, NoNodesSelected, UnderRestrained;
                             every variant carries the decl's SupportLabel and target as authored */ }
impl SupportRefusal { pub fn diagnostic(&self, cap: &SupportCapability) -> Diagnostic; }   // the one mapping
pub fn realize_supports(decls: &[SupportDecl], view: &BoundaryView, cap: &SupportCapability)
    -> Result<Realization, Vec<SupportRefusal>>;
```

These names and shapes are normative:

- the row constant names;
- `SupportTarget`'s four forms;
- `select`;
- the one mapping site;
- the rule that `Err` carries every refusal.

Variant names inside `SupportRefusal` and the Warning code's spelling are tactical (§13 OQ-2).

### §6.3 The policy table (`constraint_pattern`)

| kind | body | frame | DOFs per node | today's home |
|---|---|---|---|---|
| Fixed | Tet | any | u_x, u_y, u_z | modal `ClampAllDofs`; dims root loop; `target_node_set`; `build_support_bcs (Tet, Fixed)` |
| Fixed | Shell | any | all 6 | `build_support_bcs (Shell, Fixed)` |
| Pinned | Shell | any | u_x, u_y, u_z (rotations free: an exact hinge) | `build_support_bcs (Shell, Pinned)`, never constructed in production |
| Pinned | Tet | `BoxFace { normal }` | the two tangential translations (the hinge face, D1) | replaces modal `PinTransverse` and buckling's lateral clamp |
| Pinned | Tet | `Unframed` (a realized face) | refusal `KindNotRealizable` → `FeaSupportNotRealized` (R9) | replaces the `(Tet, Pinned)` = clamp arm and `target_node_set`'s kind-blind clamp |
| any | Tet | a `Patch` target | u_x, u_y, u_z, or the restrained local DOFs when `restrain` is given (δ, via `NodalBasis`) | task/7189 `point_support_dofs`; CT ε |

A node shared by two supports restrains the union of their DOFs, or the span of their directions for
`restrain` (CT ε's rule).

### §6.4 Resolution (`resolve_support_target`)

| target | `StructuredBox` | `Realized` |
|---|---|---|
| `Named(s)` | `BoxFace::parse(s)` (unparseable → `NotAFace`), then as `Face` | `BoxFace::parse(s)`, then as `Face`; unparseable → `TargetNotRealizable` (R10) |
| `Face(f)` | Nodes by coordinate: `coord[axis] <= eps` or `>= extent − eps`, eps 1e-9 (today's `face_nodes` predicate, the single home, D10). Frame `BoxFace { normal }`. | Only `XMin` is realizable, as the AABB x-min face by coordinate (`realized_cantilever_bc_node_sets`' predicate moves here); frame `Unframed`. Any other face → `TargetNotRealizable` (R10). |
| `Faces(h)` | `TargetNotRealizable` | the face → node map over `boundary`; an empty result → `NoNodesSelected`; frame `Unframed` |
| `Patch { nodes, .. }` | the nodes, bounds-checked against `coords`; frame `Unframed` | the same |

### §6.5 Composition (`realize_supports`)

Each step is one function (heuristic 4):

1. Capability check per decl: kind ∈ `kinds`, target form ∈ `targets`.
2. Resolve every target.
3. Face check: the `ResolvedTarget.face` ∈ `faces`.
4. If any step-1, -2 or -3 refusal exists, return `Err` with all of them.
5. Pattern per decl.
6. Emit the union per node.
7. Implicit clamp (`cap.implicit_clamp`, honouring `unless`).
8. The D3 anchor rule.
9. Restrain bases (δ / #8256).
10. Normalize: sort and dedup by dof.
11. `free_rigid_modes`.
12. Mechanism policy:
    - `Refuse` with free modes > 0 gives `Err(UnderRestrained)`;
    - `Warn` adds the coded Warning to `Realization.diagnostics`.
13. `Applied`.

### §6.6 Capability rows (all in `supports/capability.rs`, listed in `REGISTERED`)

| row | solver path | kinds | targets | faces | implicit clamp | mechanism | lands in |
|---|---|---|---|---|---|---|---|
| `ELASTIC_SHELL` | the shell plan λ #9118's planner selects (declarative: μ refuses non-root supports before routing, so the shell never sees a user support and only the matrix test drives this row) | Fixed | Named, Face | Only(XMin) | XMin, Never | Refuse | α₁ (`shell_solve.rs` calls the table) |
| `MODAL_BOX` | modal free_vibration (dims overload; #8247 only moves the list into a `supports` argument) | Fixed, Pinned | Named, Face (+ Patch after δ) | Any | — | Warn | α₂ |
| `ELASTIC_DIMS` | `solve_elastic_static` dims and `solve_load_cases` dims (shared supports refused once, per-case supports per case, as μ specifies) | Fixed | Named, Face (+ Patch after δ) | Only(XMin) | XMin, Never (PointOnly after δ) | Refuse | β |
| `ELASTIC_FIELD` | the as-printed Field overload; remedy "only the root clamp can be modelled" | Fixed | Named, Face | Only(XMin) | XMin, Never | Refuse | β |
| `ELASTIC_BODY` | the body overload and `solve_load_cases` body | Fixed (Pinned → R9 refusal) | Named, Face (XMin only), Faces (+ Patch after δ) | Only(XMin) for Named/Face | XMin (the AABB set), unless AnyApplied (D13) | Refuse | β |
| `BUCKLING_COLUMN` | `solve_buckling` P1/P2 and per-case `solve_buckling_load_cases` | Fixed, Pinned | Named, Face (+ Patch through #7081 (a)) | Any | — | Refuse | γ |
| `MODAL_BODY` | #7142's body-arg modal overload | Fixed (Pinned → R9 refusal) | Faces, Patch | — | — | Warn | added by #7142 / #7143 when they land (a note on each) |

Mechanism modal has no support input (`mechanism_modal_analysis(mechanism, options)`). Its ignored
`ModalOptions.boundary_conditions` is PDROP's business (#7079) until #8247 removes the field. The
membrane/tensegrity Int node-index lists are not `Support` declarations and are out of scope.

### §6.7 Invariants

1. **One home.** No production code outside `supports/` maps a support kind to DOFs, parses a box-face name
   for a support, places a support anchor, or declares a capability row. Ratchet rows (§6.8) and D12's coverage
   test (ε) enforce this.
2. **Totality (INV-SF-3).** For every row in `REGISTERED`, every decl is either in `applied` or is exactly one
   `SupportRefusal`. `support_capability_matrix.rs` drives every row against kind × every target form × both
   views. The reify-eval/analysis coverage test (D12, landed by ε) fails when a `List<Support>` target names no
   row.
3. **Failed, never Completed-with-Error (INV-SF-2).** A refusal leaves its entry as `ComputeOutcome::Failed`
   carrying the coded Error. This is checked at construction (`realize_supports` returns `Err`) and at use (the
   ζ₀ `completed` debug-assert).
4. **Coded (INV-SF-6).** Every refusal and Warning carries a `DiagnosticCode`. PDIAG counts no new code-less
   site.
5. **The echo is the realization.** A result's applied-supports field is built only through `Applied::select`.

### §6.8 Ratchet rows

| row | host | counts | target |
|---|---|---|---|
| `support-dof-pattern-arms` | `crates/reify-solver-elastic/tests/adoption_ratchets.rs` (production scope) | `(SupportBodyKind::` match-arm tuples outside `src/supports/` | 0 (α₁) |
| `support-anchor-nearest-node` | S5's `tests/infra/test_analysis_stratum_ratchet.py` (reify-eval/reify-analysis production; the copies are there, not in the solver crate) | `fn nearest_node\b` definitions | 0, or 1 while #7189's `point_support.rs` / #8251's `node_patch.rs` copy stands (δ or #8251 lowers it) |
| `support-face-vocabulary` | S5's member | `fn face_bound`, `fn face_company`, `fn any_support_targets`, `SYNTHETIC_CLAMP_FACE` | 0 after α₂ and β (μ may already have removed part of the elastic one) |
| `support-bc-literals` | S5's member | `DirichletBc {` constructions in the buckling, elastic and modal production families | 0 after α₂, β and γ, or 1 while #7189's `point_support_bcs` literal stands (δ lowers it to 0) |

**Who adds the S5 rows.** α₂, β and γ have no edges among themselves. Whichever lands first ADDS the three S5
rows at the values measured on its own tree, using the patterns above. Each later leaf lowers them by the sites
it deletes. This is the same first-lands rule as `support_decls` (D6). #7189 lowers nothing and must not
re-baseline: its `point_support_bcs` literal and `nearest_node` copy are retired by δ / #8251.

ο's `box-face-literal` row stays at 0. γ builds its synthesized declaration from `SupportTarget::Face(BoxFace::ZMin)`
and `Face(BoxFace::ZMax)`, never as a literal.

## §7 Boundary-test sketch

Each row faces one side of the seam: P means producer (the module), C means consumer (a trampoline).

| # | side | scenario | preconditions | postconditions |
|---|---|---|---|---|
| BT1 | P | the table is total | every (kind, body, frame) | one `DofMask` or one `PatternRefusal`. `(Pinned, Tet, Unframed)` refuses; `(Pinned, Tet, BoxFace{X})` = {u_y, u_z} |
| BT2 | P | the anchor rule | `[Pinned(x_min), Pinned(x_max)]` on a box | exactly one u_x BC, at the node nearest the analytic x_min centre; `free_rigid_modes == 0` |
| BT3 | P | mechanisms | `[Pinned(x_min)]`; `[Pinned(x_min), Pinned(x_min)]`; `[Pinned(y_min), Pinned(z_min)]` (adjacent faces) | 2, 2 and 1 free modes. `MODAL_BOX`: `Ok` plus one coded Warning. `BUCKLING_COLUMN`: `Err(UnderRestrained)` naming the count |
| BT4 | P | totality matrix | every `REGISTERED` row × kinds × every target form × both views | each cell is applied or exactly one refusal (§6.7 inv 2) |
| BT5 | P | Fixed is unchanged | each committed Fixed configuration | the exact DOF set equals today's (modal clamp, dims root loop, AABB clamp including the empty list, shell root): exact-set tests ported from the old functions before they are deleted |
| BT6 | C, modal | the hinge measured | `simply_supported_beam_modes.ri` (2% analytic bands) and `clamped_clamped_beam_modes.ri` (its e2e bands); `printer_gantry_modes.ri` and `printer_print_envelope.ri` (structural e2e only, no analytic tolerance; `printer_print_envelope.ri` is observable in process only because `reify eval` fails on #5847's constructor debt); `prj/printer_v01/printer.ri` | the banded ones stay inside their bands; the structural e2e tests stay green; before/after frequencies of every one recorded in α₂'s commit; `reify check prj/printer_v01/printer.ri` verdicts recorded before/after if crash #7383 has been fixed, else its two pin-pin modal cells recorded as unmeasured |
| BT7 | C, modal | the lone pin is reported | `support_policy_modal_lone_pin.ri` (546 DOFs); a copy above the 1024-DOF ceiling built in the test | below the ceiling: `reify eval` prints the realization Warning naming 2 free modes, plus `W_ModalRigidBodyMode` for modes 0 and 1 (predicted from the rank, not yet measured). Today it prints f1 = 18925.97 Hz plus `I_ModalPinnedFaceRealization`. Above the ceiling: the realization Warning, then Failed with `E_ModalNoModesComputed` |
| BT8 | C, modal | refusal through the module | `support_policy_modal_unknown_face.ri` | exit 1, `FeaSupportNotRealized` by code, the support's span label. Today: rc 0, f1 = 0 Hz, three `W_ModalRigidBodyMode` |
| BT9 | C, elastic | dims rows | μ's five fixtures | unchanged exit codes and codes (`FeaSupportNotRealized` per R1) |
| BT10 | C, elastic | body refusals and the kept clamp | `support_policy_body_tip_target.ri`, `support_policy_body_pinned_root.ri`, and an empty-list body scene, in process with gmsh | the two fixtures: Failed, one `FeaSupportNotRealized` each, naming the support. Today: the gmsh path clamps AABB x-min (before-image recorded in β). The empty list: unchanged AABB clamp plus `FeaUnderConstrained` |
| BT11 | C, elastic | static numbers | every committed static example (S16 BT3's set) | identical printed results |
| BT12 | C, buckling | the hinge measured | `buckling_column_smoke.ri`, `buckling_column_p2.ri`, `buckling_multi_case_smoke.ri`; `euler_column_pin_pin.rs` pin-pin and fixed-pin through `realize_supports` | P_cr inside the existing bounds (P1 10%, P2 5%); before/after recorded in γ's commit; fixed-free and fixed-pin unchanged |
| BT13 | C, points | δ | 7189's committed fixtures | values and exit codes unchanged; one fixture whose PointSupport is applied on both static and modal |

## §8 Cross-PRD relationship (G4)

| artefact | owner | this PRD's role | status |
|---|---|---|---|
| support realization (`constraint_pattern`, `resolve_support_target`, `realize_supports`, `SupportCapability` rows) | **this PRD** (program G4 row) | owner | α₁ |
| `SupportSpec` decode | S15 γ #8683 | consumer, unchanged until δ adds the point form (a follow-on edit); no write to #8683 (β edits its landed T8 body-path test as a follow-on, G1) | upstream edge (α₂) |
| `BoxFace`, `StructuredBoxMesh` | S16 ο #9121 | consumer; `nodes_on` delegates to the module's predicate (D10) | upstream edge (α₁) |
| modal `realize_supports` (modal-local, K5-3/K5-4) | S16 ξ #9120 | absorbs it as a call in α₂ | upstream (via ο); ξ is not re-scoped |
| dims refusal (`FeaSupportNotRealized`) | S16 μ #9119 | turns it into the `ELASTIC_DIMS` row in β | upstream edge; code renamed by R1 (three writes) |
| route planner, shell plan | S16 λ #9118 | none; `ELASTIC_SHELL` names its plan | never waits on this stream |
| `ComputeOutcome` (Error ⇒ Failed) | S5 + S15 ζ₀ #9287 | consumer | upstream (transitively) |
| `SolveBoundary`, `node_patch`, `nodal_basis`, `FeaPointSupportsUnderRestrained` | coordinate-target-fea (#8253, #8251, #8256) | consumes `patch_nodes` and `NodalBasis`; never edits them; δ folds the point code (D4) | δ ← #8251; #8256 adapts second (R7) |
| PointSupport realization sites | #7189 (unlanded branch) | absorbed by δ after it merges | δ ← #7189; note only on #7189 |
| per-axis DOF masks on point anchors | #7213 | expressible as `Patch.restrain` (#8256) | note only |
| selector-typed modal supports; body-arg modal | #7143, #7142 | `Faces` on `Realized`; they add `MODAL_BODY` | notes; they adapt second |
| body-overload adaptive fallback | #8248 (in the closure) | shares `elastic_static`'s `bc_override` path with β | whichever lands second adapts; no edge, no note |
| buckling support honouring | #7081 | a call to `realize_supports` with `BUCKLING_COLUMN` | #7081 ← γ, ← #8683 (rewritten) |
| v0_3 modal-analysis PRD pin prose | #7871 | its prose must describe the hinge | #7871 ← α₂ (note) |
| trampoline placement (reify-analysis) | S5 κ₁ #8999 / μ #9003 / ν #9004 | modal settled (α₂ after μ #9003); elastic adapts by grep | no edge |
| solver-crate adoption-ratchet binary | S14 (#8614, done) | adds two rows | — |
| reify-eval/reify-analysis ratchet member | S5 α #8984 | adds two rows | upstream through ο |
| INV-SF-3 text (one home: design-invariants.md) | S24 / S12 (one home per id) | in-place amendment in ε (R12) | ε |
| FEA doc chunk `crates/reify-mcp/src/tools/chunks/fea.md` | first creator among μ #9119, #8259, #7088, #8296 and solver-performance λ #8384; the others extend it | ε extends it (never creates it): a supports section | ε ← β ← μ |

No reciprocal "the other owns it" remains.

S16's §12/§13 still spell R3's code `FeaDimsSupportNotRealized`. That is a cross-reference line to the S16
lead; this session edits only μ's delivered-check twins and task record (R1).

## §9 Decomposition plan

| label | id | title | prio | depends on | user-observable signal |
|---|---|---|---|---|---|
| **α₁** | #9379 | One support-realization module in reify-solver-elastic: table, resolver, composer, rows, rank helper; shell realizes through it | medium | #9121 | `cargo test -p reify-solver-elastic` runs `support_capability_matrix` over `REGISTERED`; the shell examples print byte-identical results (BT5) |
| **α₂** | #9380 | Modal realizes through the module; the hinge pin; typed applied echo | medium | α₁, #8683 | `reify eval tests/prd-gate/fixtures/support_policy_modal_lone_pin.ri` prints the coded mechanism Warning naming 2 free modes (binding) and, predicted, `W_ModalRigidBodyMode` for modes 0 and 1 (today f1 = 18925.97 Hz, a clamp); pinned modal examples inside their bands, numbers recorded |
| **β** | #9381 | Elastic dims, Field and body realize through the module; μ's refusal becomes the dims row; body refusals (R9, R10) | medium | α₁, #9119 | in process with gmsh, `support_policy_body_tip_target.ri` and `support_policy_body_pinned_root.ri` give Failed with `FeaSupportNotRealized` naming the support; every committed static example prints identical results |
| **γ** | #9382 | Buckling realizes through the module (`BUCKLING_COLUMN`; synthesized pin-pin); #7081 becomes a call | medium | α₁ | `reify eval examples/buckling_column_smoke.ri` prints a critical load inside the 10% band with the centroid anchor, before/after recorded; `euler_column_pin_pin` families through the module, inside their bounds |
| **δ** | #9383 | Point patches as a target form: 7189's realization sites and its rigid-body guard absorbed; the restrain seam; the point code folded | medium | α₂, β, #7189, #8251 | 7189's committed fixtures print unchanged values and exit codes; a PointSupport is in `applied` on both static and modal |
| **ε** | #9384 | INV-SF-3 covers supports; per-solver refusal e2e; FEA chunk and exemplar | **high**, gate #9171 | α₂, β, γ | `support_refusals_e2e.rs` shows modal and elastic dims each return Failed with `FeaSupportNotRealized` on a committed fixture, and the body case does the same in β's gmsh binary; the SF-3 census row reads shipped |
| **Ω** | #9385 | PRD close: SHIPPED header, landed ids, AS-AUTHORED freeze | low | α₁–ε | the committed header |
| (M) | #9386 | Out of plan: decision milestone — rigid-coupling hinge (option D) | low | Ω, #8612 | Leo's recorded decision; a follow-on /prd if D is chosen |

**Edges onto other tasks:**

- `#9171 ← ε` (survey gate; high);
- `#7081 ← γ` and `#7081 ← #8683` (§11);
- `#7871 ← α₂`.

All were checked against the live #8612 closure (240 on 2026-10-18; 239 on 2026-10-19, the filing day). No closure task depends on any leaf here, and
none of the edges' dependents is in the closure.

### Filing (stamps and landing; brief §Stamps, §Landing)

- **Every leaf:** `source: "hotspot-survey"`, `x_finding_run: "hotspot-survey-reify-20261015"`, `prd_path`,
  `prd_task_label` (the sidecar label byte-for-byte: `α₁`, `α₂`, `β`, `γ`, `δ`, `ε`, `Ω`),
  `user_observable_signal`, `consumer_ref`, `grammar_confirmed: true`, `metadata.files` (tight-or-empty, through
  `scripts/lock-charter-guard.sh`), `task_kind: "normal"`.
- **`metadata.files`.** The files of α₂, β, γ and δ move before they run (S5 ν/μ, S16 θ/ν), so their lists name
  only stable paths. `crates/reify-solver-elastic/src/supports/capability.rs` is deliberately on α₂, β and γ:
  it is the serialization lock among the three row-adding siblings. The architect acquires the rest.
- **`x_gapfill_ruling`:** `"plans/gap-fill-deliberation-reify-20261016.md §K5-2"`. α₂ also cites
  `§Uncontested block → K5-3, → K5-4`.
- **Keys and priority (§10):**
  - α₂: `x_finding_key [fk-ec25b28cba70, fk-5edeea9c474a]`, medium;
  - α₁, β, γ, δ: `x_finding_part [fk-47257bcb2c8c]`, medium;
  - ε: `x_finding_key [fk-47257bcb2c8c]`, **high**, gate `#9171 ← ε`;
  - Ω: low.
- **M:** `execution_class: "decision"`, `task_kind: "deterministic"`, `always_escalates: true`, no
  `prd_task_label`.
- **Before the commit:**
  - #8612's closure recomputed the same day;
  - #9119's three writes done;
  - #7081 rewritten;
  - the notes written;
  - `plans/gap-fill-deliberation-reify-20261016.md` confirmed tracked;
  - the FILED row appended under `## FILED — gap-fill streams`;
  - the stamped sidecar committed with the PRD, the `.md` twin, the four fixtures, the S16 twins and the
    record, in one `git commit --only`.

### Leaf detail

**α₁ — the module** (x_finding_part fk-47257bcb2c8c). This leaf touches only reify-solver-elastic, plus a
reify-core code. Its numbers are byte-identical.

- **The module.** §6.1–§6.5, with the target forms `Named` and `Face`, the rank helper, `nearest_node` (moved),
  and the `ELASTIC_SHELL` row in `REGISTERED`.
  - β adds `Faces`, because the face→node map is in reify-eval (§13 OQ-1).
  - δ adds `Patch`, and whichever of δ / #8256 lands second adds `Realization.bases`.
  - `shell_boundary.rs` is absorbed, and `shell_solve.rs` calls `constraint_pattern(Fixed, Shell, …)` for its
    root edge. Its drilling θz pins are not supports and stay.
  - `build_support_bcs` and `SupportCompatibility` are deleted. Their unit tests move to the table tests.
  - ο's `StructuredBoxMesh::nodes_on` delegates to the module's face predicate, or is deleted if unused (D10).
- **reify-core** (R4, D9). Add it to `Cargo.toml` and amend the `diagnostics.rs` header rule.
  - Mint `FeaSupportsUnderRestrained`.
  - Mint `FeaSupportNotRealized` if neither ξ nor μ has. ξ precedes α₁ through ο, so normally it exists.
- **Tests.**
  - `tests/support_realization.rs`: BT1–BT3, driven through the public seam.
  - `tests/support_capability_matrix.rs`: BT4 over `REGISTERED`. Its census row lands in
    `docs/legibility/design-invariants.md` in this diff as `chartered`, since it adds a declarable surface.
  - The solver-host ratchet row `support-dof-pattern-arms` (the S5-member rows are §6.8's first-lands rule).
  - nextest partition rows for the two new test binaries where applicable (gate-test drift-guard rule); no
    wall-clock bounds.
- **Done when:** BT1–BT5 green for the shell; the delivered checks hold; `python3 scripts/assert-module-scc.py
  check` reports no GREW.

**α₂ — modal** (x_finding_key fk-ec25b28cba70, fk-5edeea9c474a; x_finding_part fk-47257bcb2c8c). This leaf runs
in reify-analysis's modal family; locate it by grep.

- **Adopt the module.** ξ's modal-local `realize_supports` becomes the adapter:
  - `support_decls` (D6) feeds the module with `MODAL_BOX`;
  - `Err` → `Failed` carrying one Diagnostic per refusal;
  - `applied.select(&specs)` → `Applied<Vec<SupportSpec>>` for ζ's `ModalResultParts.boundary_conditions`
    (D7);
  - the realization's Warning (D4) joins the outcome's diagnostics.
  - The `MODAL_BOX` row goes into `REGISTERED`.
  - These die: `face_bound`, `is_beam_axis_end_face`, `face_company`/`FaceCompany`,
    `face_realization`/`FaceRealization`, `per_face_bcs`, `face_nodes`, `normalize_bcs`,
    `end_face_neutral_axis_node`, `pin_pin_axial_anchor`, modal's `nearest_node` copy,
    `PINNED_BEAM_END_REALIZATION`, `pinned_end_face_realization_diagnostics`, and the
    `I_ModalPinnedFaceRealization` notes (D1).
  - ξ's supports tests are rewritten against the module's public seam (the Tests stance). Fixed configurations
    are exact-set ports. The pinned configurations become hinge sets with their free-mode counts: pin-pin,
    propped cantilever, lone pin, duplicates, non-end pin.
- **Measure (D11, BT6).** Before deleting the old functions, record every pinned modal configuration's
  frequencies under both realizations, in the commit message.
  - Banded configurations (`simply_supported_beam_modes`, `clamped_clamped_beam_modes.ri`) are re-baselined
    inside their existing analytic bands.
  - The structural-only examples (`printer_gantry_modes`, `printer_print_envelope`) keep their e2e tests green
    and are recorded in process. `printer_print_envelope.ri` fails `reify eval` on #5847's constructor debt.
  - `prj/printer_v01/printer.ri`: if `reify check` completes (crash #7383 fixed), record its verdicts before
    and after. If it still crashes, say so in the commit and record its two pin-pin modal cells as unmeasured.
  - If a band is crossed, or a printer.ri verdict flips, stop and escalate with the measurement. Never widen a
    band.
  - Cantilever and transient examples (Fixed) must print byte-identical frequencies.
- **Rows and registration.**
  - The S5 rows: add them if α₂ lands first of α₂/β/γ (§6.8), otherwise lower them. Modal's
    `support-face-vocabulary` sites, its `nearest_node` copy and its two `DirichletBc {` sites leave.
  - Register `support_policy_modal_lone_pin.ri` and `support_policy_modal_unknown_face.ri` wherever a Rust test
    reads them (`_RUST_COUPLED_RI_FIXTURES` until S18 ζ #9112).
- **Done when:** BT6–BT8 green; the delivered checks hold.

**β — elastic** (x_finding_part fk-47257bcb2c8c). The adapter is `support_decls` (D6); add it if α₂ has not.

- **Dims and Field.** `ELASTIC_DIMS` and `ELASTIC_FIELD` replace μ's request-validation refusal arm, with the
  same remedies and the same labelled Errors under `FeaSupportNotRealized`. The dims root clamp is read from
  `Realization.bcs` instead of the inline root loop.
  - The empty-list `FeaUnderConstrained` Warning and its structured detail are unchanged
    (`applied.is_empty() && implicit_clamp_applied`).
  - `SYNTHETIC_CLAMP_FACE` and `any_support_targets` die, if μ has not already retired them. The inline root
    loop's `DirichletBc {` leaves `support-bc-literals` (§6.8).
- **Body.** `ELASTIC_BODY` (D13) is `Fixed` only, with the AABB x-min implicit clamp `unless: AnyApplied`.
  - `Named("root"|"x_min")` and `Face(XMin)` resolve to the same AABB set.
  - β adds `SupportTarget::Faces` and its matrix cells. It decides §13 OQ-1 (suggested: move
    `bc_resolve::boundary_node_set` to `reify_ir::BoundaryAssociation::nodes_on_faces`, with bc_resolve
    delegating). A `Faces` union stands alone, as today (it replaces the AABB clamp). An empty result keeps
    `FeaSelectorNoMatch` byte-stable.
  - A Pinned decl or any other Named target is refused (R9, R10).
  - The supports half of `loads_supports_to_bc_node_sets` / `target_node_set` and the root half of
    `realized_cantilever_bc_node_sets` die. The loads side stays, renamed to its loads purpose.
  - Today's `bc_override` (or `SolveBoundary.node_override`, if CT β #8253 has landed) carries the module's node
    set, consumed only. `apply_face_clamp` is read from `implicit_clamp_applied`.
- **Rewrites:**
  - `loads_supports_to_bc_node_sets_unions_multiple_support_faces` keeps the union with Fixed handles;
  - the PinnedSupport `SelectorNoMatch` label test becomes the R9 refusal;
  - #8683's landed T8 body-path Pinned assertion becomes the refusal. This is a follow-on edit of a landed
    test; there is no write to #8683.
- **Tests and fixtures.**
  - An in-process test in `crates/reify-eval/tests`, the gmsh binary (P7), drives the two body fixtures and an
    empty-list body scene (BT10). The gmsh before-image (max von Mises and the applied clamp set under today's
    silent AABB clamp) goes in the commit message.
  - The fixtures are registered for PG-DRIFT.
  - S5 rows per §6.8's first-lands rule; the elastic root loop's `DirichletBc {` and the elastic
    `support-face-vocabulary` sites leave.
- **Done when:**
  - BT9–BT11 green;
  - μ's five fixtures keep their exits;
  - the delivered checks hold;
  - the S5 `support-face-vocabulary` row is at 0.

**γ — buckling** (x_finding_part fk-47257bcb2c8c).

- Replace the P1 `for k_face in [0usize, nz]` loop with its anchor, and the P2 coordinate loop with
  `DirichletBc { dof: 2 }`. The replacement is one `realize_supports` call on a synthesized
  `[Pinned(Face(ZMin)), Pinned(Face(ZMax))]` under `BUCKLING_COLUMN`, over `BoundaryView::StructuredBox { coords,
  extent: [lx, ly, lz] }`. `coords` is `&nodes` in the P1 arm and `&nodes_p2_storage` in the P2 arm;
  `active_nodes` is bound only after the kernel call.
  - The module's 1e-9 face predicate selects the same end-face set as today's 1e-10 P2 test. Assert the set
    equality in the port.
  - The axial anchor moves to the z_min centroid (D1, D3). On both grids the centroid is a node: P1 i = j = 4,
    P2 i = j = 1.
  - Keep `let _ = &value_inputs[5];` and its comment verbatim. Its "Support-driven BC selection is deferred"
    phrase is #7081's delivered check, so repoint only the "(no live task)" cite to `#7081`.
- `BUCKLING_COLUMN` is `Refuse` for mechanisms. The matrix test drives a lone pin → `FeaSupportsUnderRestrained`.
- **Measure (BT12).** The pre-filing measurement of `pin_pin_euler_column_within_ten_percent` with the centroid
  anchor is 9.21% (corner anchor 9.17%; bound 10%). It is recorded in the manifest's `calibration-floor`
  binding.
  - The file header's own "Observed 9.21%" predates #4052/#4066. Re-measure both anchors on your tree and
    update the header figures in the port.
  - Re-baseline the two numeric embeds (`buckling_smoke.rs`, `buckling_persistent_cache_round_trip.rs`) and the
    P2 smoke inside their existing bounds.
  - Port `euler_column_pin_pin.rs`'s pin-pin and fixed-pin families to build their BCs through
    `realize_supports`, and record the measured errors against their bounds in the commit.
  - Fixed-free, the two fixed-guided families (P1 at nx = ny = 10, and P2) keep their hand-built sets:
    fixed-guided needs MPC rows no declaration expresses.
  - A crossed bound is an escalation.
- S5 rows per §6.8's first-lands rule; the buckling loops' `DirichletBc {` sites leave `support-bc-literals`.
- **Done when:** BT12 green and the delivered checks hold.

**δ — points** (x_finding_part fk-47257bcb2c8c). This leaf runs after #7189 merges and CT α #8251 extracts
`patch_nodes`.

- Add `SupportTarget::Patch { nodes, restrain }` (with its resolution and its matrix cells) unconditionally.
  Add `Realization.bases` only if CT ε #8256's `NodalBasis` exists on your tree; otherwise #8256 adds `bases`
  and the `restrain → NodalBasis` routing when it lands. `NodalBasis` stays #8256's.
- **The point decode.** #8683's `SupportSpec` decodes a `PointSupport` to `UnknownVariant`. Extend the landed
  decoder with a point form (`at`, `radius`, `restrain`) unless #7189's rebase or #8256 has already added one.
  This is a follow-on edit, with no write to #8683. `support_decls` resolves it through `patch_nodes` /
  `check_on_body` on the mesh being solved, every call (CT I1), to `Patch`. `FeaPointOffBody` stays the
  caller's.
- These die: `point_support_bcs`, `point_support_dofs`, `dirichlet_nodes`, `has_face_class_support`, and the
  `apply_face_clamp` computation (it becomes `ImplicitClamp { unless: PointOnly }` on `ELASTIC_DIMS` and
  `ELASTIC_BODY`). 7189's `unrestrained_rigid_body_modes` becomes the module's rank helper.
- Fold `FeaPointSupportsUnderRestrained` into `FeaSupportsUnderRestrained` (D4). Hand-repoint #8251's
  done-producer check (three writes) and any #8256 assertion by identity.
- If CT ε #8256 has landed, route `restrain` through `Patch.restrain` → `NodalBasis`; otherwise #8256 adapts
  second (R7).
- **Done when:** BT13 green; `support-bc-literals` and `support-anchor-nearest-node` read 0 (#7189's
  `point_support_bcs` literal and `nearest_node` copy are retired here, unless #8251 already moved the latter);
  the delivered checks hold.

**ε — invariant, e2e, docs** (x_finding_key fk-47257bcb2c8c; high; gate #9171).

- **INV-SF-3, amended in place in `docs/legibility/design-invariants.md`** (heading byte-identical; R12):
  - the Rule names "a load or support declared on a solve";
  - the house pattern names the capability row plus one-refusal-per-unrealizable-support;
  - an `**Enforcement**:` paragraph in S24's pointer grammar points at
    `crates/reify-solver-elastic/tests/support_capability_matrix.rs`, D12's coverage test,
    `crates/reify-eval-fea-tests/tests/support_refusals_e2e.rs` and β's gmsh body test;
  - α₁'s census row moves to `shipped`.
- **D12's coverage test** (reify-eval/analysis side): every production target whose stdlib declaration takes
  a `List<Support>` names a row in `REGISTERED` by its `solver` id. It lands here because ε depends on every
  row-adding leaf (α₂, β, γ).
- **`support_refusals_e2e.rs`** (reify-eval-fea-tests, no gmsh): modal box (`support_policy_modal_unknown_face.ri`)
  and elastic dims (μ's `dims_support_xmax_only.ri`) each return Failed with `FeaSupportNotRealized` by code
  and name the support. The body family is β's gmsh test. Buckling joins when #7081 honours its list.
- **Docs-truth.**
  - Extend `crates/reify-mcp/src/tools/chunks/fea.md` (never create it; §8) with a supports section: what
    Fixed and Pinned mean (the hinge), per-solver capability, the refusal, and the lone-hinge mechanism,
    including that above the dense ceiling a modal lone hinge is a failed solve.
  - The section is findable by intent ("why is my PinnedSupport refused", "my pinned beam has rigid-body
    modes"). Each signature compiles in a smoke `.ri`.
  - A `examples/best_practices/` exemplar of a simply supported beam with two hinges, plus its `INDEX.md` line.
  - A one-line index entry in `.claude/skills/reify-design/SKILL.md`.
- **Done when:**
  - the heading is byte-identical (grep);
  - the `**Enforcement**:` paragraph names the e2e files (the delivered check);
  - the census row reads `shipped`;
  - `support-bc-literals` reads 0, or 1 while #7189's `point_support_bcs` literal stands (δ lowers it);
  - D12's coverage test is green;
  - the e2e and the chunk smoke are green.

**Ω — PRD close.** Follows the overlay's "PRD terminal status" rule: SHIPPED header, landed leaf ids, AS-AUTHORED
freeze and LIVE map, and the same header on the `.capability-manifest.md`.

**M — rigid-coupling hinge milestone** (out of plan; R8). A decision gate (`execution_class: decision`). On
dispatch, escalate to Leo:

- (a) keep the hinge face (D1);
- (b) author a /prd for a distributing-coupling hinge: the face is coupled to a centroid reference node, the
  reference node's translations are pinned, its rotations are free;
- (c) partial.

It depends on Ω and #8612.

### G7 walk (reify INV-SF-1..6)

- SF-1: a refused entry's cells are Undef with `SolveFailed` through `mark_failed`.
- SF-2: every refusal is Failed (§6.7 inv 3).
- SF-3: extended (ε).
- SF-4: n/a.
- SF-5: no placeholder. δ's `Patch` resolution lands with its producer, and `MODAL_BODY` is added by its owners.
- SF-6: every new diagnostic is coded (`FeaSupportNotRealized`, `FeaSupportsUnderRestrained`, the modal
  mechanism Warning); PDIAG baselines are never raised.

No waiver.

## §10 Finding-key disposition

| key | leaf |
|---|---|
| fk-47257bcb2c8c (high) | ε (key, gate #9171); α₁, β, γ, δ (parts) |
| fk-ec25b28cba70 | α₂ (key; R5) — the cross-solver form of ξ's modal totality |
| fk-5edeea9c474a | α₂ (key; R5) — `Applied<T>`, the typed form of ξ's applied echo |

## §11 Amendments to filed tasks (made at filing)

- **μ #9119:**
  - code `FeaSupportNotRealized`: mint it if ξ has not; R3's behaviour is unchanged;
  - three writes on `dims-support-code`: the S16 sidecar `.yaml`, its `.md` twin, and #9119's complete
    `metadata.delivered_checks`;
  - `user_observable_signal` updated.
  - Under R1 that check no longer discriminates μ from ξ; μ's other four checks still do.
- **ξ #9120:** a note. The modal refusal code is `FeaSupportNotRealized` (mint it if μ has not). α₂ later absorbs
  `realize_supports` and changes the pin to the hinge (R3); ξ keeps today's numbers.
- **#7081:** a full-text rewrite.
  - "Port modal's `build_dirichlet_bcs`" becomes "call `realize_supports` with `BUCKLING_COLUMN`".
  - Rename the examples' `"base"` targets.
  - Delete the discard line.
  - A lone hinge is `FeaSupportsUnderRestrained`.
  - PointSupport/restrain amendment (a) arrives through δ's `Patch`, or as `FeaSupportNotRealized` until δ.
  - B8's ratio uses γ's pin-pin number.
  - Every existing clause is ported. Edges: #7081 ← γ, #7081 ← #8683.
- **#7871:** edge #7871 ← α₂, plus a note: describe the hinge (R3), because `PinTransverse` and the lateral
  anchor die.
- **Notes (no scope change):**
  - #8256: adapt second; `restrain` → `Patch.restrain` → `Realization.bases`; the rank rule is the module's
    helper; the point code folds (D4).
  - #7143 and #7142: `Faces` on `Realized`; they add `MODAL_BODY`; Pinned refused (R9).
  - #7189: δ absorbs its sites after merge.
  - #8251: `nearest_node`'s home; its done-producer code check is repointed by δ.
  - #7213: per-axis masks are `restrain` directions (#8256).
  - #8649 (S22 ε, which reworks binary resolution in `scripts/prd-capability-check.py`): `_resolve_reify_bin`
    prefers `target/release/reify` even when it is weeks older than `target/debug/reify`. This session's
    decompose-verify run got body-overload evidence from the stale release binary.
- **No write to #8683** (in the closure). β edits its landed T8 test as a follow-on.

## §12 Out of scope

- **New dims boundary-condition semantics** (S16 R1). `ELASTIC_DIMS` realizes exactly today's root clamp.
- **The exact hinge (option D).** A distributing coupling needs MPC machinery that has no production caller
  (`mpc.rs` leaves K unsymmetric). It is milestone M (R8).
- **Directional restraint (`NodalBasis`, rotate/back-rotate plumbing)** is coordinate-target ε #8256's. This
  module carries `bases` only.
- **Mechanism-modal supports** (PDROP #7079; #8247).
- **Membrane/tensegrity node-index supports.**
- **Load realization** (`LoadSpec`, point loads, pressure faces). This includes ο's `box_face_plane` typo
  question.
- **Fixed-guided** (an MPC family) as a declaration.
- **The CLI body overload's no-gmsh fallback** (P7) is observed here, not fixed.

## §13 Open questions (tactical)

1. **Where the face → node map lives.** Options:
   - move `bc_resolve::boundary_node_set` down to `reify_ir::BoundaryAssociation::nodes_on_faces` (meshing-service
     ε #8288 then extends it in place; a cross-reference line to #8288/#5313);
   - have `BoundaryView::Realized` carry a resolver callback.

   Suggested: the move. Decide in β.
2. **Spellings:** the `SupportRefusal` variant names and the modal mechanism Warning's code (D4). Decide in α₁
   and α₂.
3. **The rank helper's tolerance.** Scale it to the coordinate extent; 7189's Gram uses one. Decide in α₁; δ
   reconciles with 7189's.
