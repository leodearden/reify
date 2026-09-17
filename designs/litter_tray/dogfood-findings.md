# Dogfooding findings — litter-tray session, 2026-07-14

Nine findings from designing the bottom deck, plus one papercut found during
triage. All investigated by a 7-agent Opus team on 2026-07-14; verdicts and
fix tasks (filed as reify tasks 5193–5206) below. None had live task
coverage before this session.

## 1. ~~No selective edge fillet~~ — FINDING WAS STALE
3-arg `fillet(solid, edges, radius)` + curated chamfer are wired end-to-end
(compiler → IR → eval → OCCT; tasks #3205/#4360/#4362/#4185; active e2e
`fillet_curated_edges_3205_e2e`). The session was misled by the stale header
of `examples/topology_selectors/fillet_top_edges.ri` (claims the gap still
exists; the same repo's `#[ignore]` prose already says it landed).
→ **5203** fix stale header; **5201** `rounded_box`/`rounded_rect`
convenience primitives; **5202** edge-class selector sugar
(`top_edges`/`bottom_edges`/`vertical_edges`/`concave_edges`/`convex_edges`).
Design follow-up: retry the tray with 3-arg fillets + `concave_edges`.

## 2. Intermediates flood the viewport — default-hide rule EXISTS but is doubly dead
Leo remembered right. `autoViewGenerator.ts:56` hide rule matches
`Solid|Surface|Curve` but the emitted type string is `"Geometry"`
(`ty.rs:601`) — regex never matches; and meshes key on sibling
`kind:"realization"` nodes (#4954 two-node shape) that the `let` rule never
touches — only `aux` realizations get `default_visible=false`
(`engine.rs:5039-5053`). `DisplayOutput` routes but doesn't suppress.
Decision: intermediates default **hidden**; DisplayOutput = explicit routing.
→ **5195**.

## 3. Rigid mass-properties fail — GUI-PATH-ONLY + a masking diagnostic bug
CLI `reify eval` computes the exact same file fine (mass 1.407 kg, full MoI).
The GUI session (loaded via debug `open_file` → `update_source`) leaves the
cells Undef — likely the same identity seam as finding 4; re-test after 5193.
Separately: "op contract failed (OpContractViolation)" is a misattribution —
`record_op_contract_failures` re-evals kernel-query cells through a
kernel-less evaluator and overwrites the true cause (gap explicitly deferred
by #4323). → **5194** (GUI path, depends on 5193), **5197** (KernelQueryFailed
undef cause; refuse-loudly policy on invalid BREPs, no silent healing).

## 4. Diagnostics cite the wrong file after debug open
Debug-MCP `open_file`/`load_fixture` funnel through `update_source`
(`debug_server.rs:1372`), which deliberately preserves the previous file's
`module_name`/`file_path` (per #3370) — new content evaluates under the old
identity; diagnostics mislabel AND jump-to-error activates the stale tab.
Normal File>Open is unaffected (`load_file` path). → **5193** (swap to
`load_file` + shared open-helper so the two paths can't drift again).

## 5. Primitive anchoring: 3 conventions, documented nowhere user-facing
box/sphere/torus centred at origin; cylinder/cone (and tube?) base at z=0;
wedge min-corner at origin. `box_centered`/`cylinder_centered` exist (#4159)
with **0 usages** — undiscoverable; ~46 `translate(primitive())`
hand-centering workarounds in corpus. Decision: document, don't unify.
→ **5204** (chunk anchor table + surface the `*_centered` variants).

## 6. 57 warnings on a healthy model — internal bookkeeping on the designer channel
"topology correspondence dropped" = expected signature of overlapping
`union_all` seams; "tied local_index at 0 and 0" fires with equal indices
where no shuffle is even possible (missing `idx_i != idx_j` guard;
`local_index` is per-primitive-relative so unions mass-produce collisions);
and none of it matters when the design has zero consuming selectors.
→ **5196** (guard + selector-consumption gating + Info downgrade +
per-realization summarization; includes checking persistent-naming-v2 intent
on group-unique indices).

## 7. Float display noise
Real 1-ulp artifacts of SI-canonical storage (0.0064 m × 1000 =
6.3999999999999995), printed shortest-round-trip with no rounding
(`format_display_number`, `value.rs:2753`). 12 sig figs cleans all observed
values. → **5198**.

## 8. Unit display gaps
`to_display_units` (`dimension.rs:424`) covers 6 dimensions, literal `"SI"`
fallback for everything else (density, pressure…); CLI uses a different
formatter (raw SI + composed dimension symbols); no auto-scaling or display
preference. → **5198** (mechanical: rounding + composed base-unit fallback +
constraint pretty-print `material[density]` → `material.density`);
**5199** GUI per-cell unit picker; **5200** source-level preference design
(depends on 5199).

## 9. `structure` vs `structure def` canon drift (corrected)
`def` is optional for structure/occurrence only; **`enum def`/`trait def` do
not exist** (bare is correct there — original finding was wrong on that).
Both structure forms are equal-status; corpus ~76% def / 24% bare;
bracket.ri (de-facto canonical example) is bare; sidecar prompt also teaches
`&& || !` where canon is `and/or/not`. Decision: `structure def` canonical,
keep alias, no deprecation warning. → **5205**.

## 10. OCCT STEP chatter in `reify eval` stdout (new, found during triage)
"Statistics on Transfer (Write)" banner + ANSI codes from the OCCT STEP
writer pollute eval output between diagnostics and cell values. → **5206**.

---
Design-session by-products: bottom deck mass 1.407 kg PETG (CLI-verified),
centroid z = 21.5 mm, capacity ≈ 7.05 L. Module-decl warning: the file wants
`module bottom_deck` at top (W_MODULE_DECL_MISSING) — fixed in the restored v1.

## Addendum — v2 selector testbed (same day, after task filing)

**Finding 1 verdict FLIPPED BACK.** Rewriting the tray with 3-arg fillets +
`edges_parallel_to`/`edges_at_height` (bottom_deck_v2_selectors.ri) fails in
the production pipeline on a fresh build containing all the landed commits:
- CLI: 6× "curated edge selection is not yet available on the current build
  pipeline … [edge selector evaluated to Undef]" (error text still cites
  4360/4358 as pending — they're done) + unresolvable GeomRef::Sub/Step.
- GUI: SILENT — determined geometry handles, zero diagnostics, zero kernel
  dispatches, sharp-cornered blanks in the viewport.
The green e2e (`fillet_curated_edges_3205_e2e`) enters through a different
seam than .ri-source compile. Classic phantom capability. → **5208** (high).
Fallout: 5202 now depends on 5208; 5201 amended to boolean-compose lowering;
5203 (already done+merged within the hour!) now overstates capability — its
header re-truing folded into 5208's scope. Working v1 restored as
bottom_deck.ri (now with `module bottom_deck`); v2 kept as the 5208 repro.

## 11. Watcher evaluates mid-write on non-atomic writes (new)
`printf > f; cat >> f` left the engine holding just "module bottom_deck" with
no re-fire on the append and no diagnostic; manual open_file recovered.
→ **5209** (debounce + re-read/self-heal).

## 12. `rotate()` docs drift + orientation type-inference papercut (new)
stdlib.md documents `rotate(geo, axis, angle)` (3-arg); reality is 2-arg
(geo, Orientation) or 5-arg unpacked (geo, ax, ay, az, angle). And
`orient_identity()` in sub placement warns "cannot infer return type of
zero-arg function, defaulting to Real" on every eval. Folded into the docs
sweep (5204 covers stdlib doc accuracy) — flag both to its implementer.

## 13. GUI segfaults on the split assembly; CLI evaluates it fine (new)
bottom_deck_split.ri (two posed instances of a heavy-CSG Rigid half, one
rotated 180° via orient_axis_angle placement) briefly RENDERS both exploded
halves, then reify-gui dies with SIGSEGV — post-first-render pass. Fresh
launch and open_file both crash; `reify eval` is clean (0.761 kg/half).
The earlier silent GUI death (watcher-reloading the v2 selector file) may be
a second trigger of the same fragility. Minimal untested probe:
probe_asm.ri (two 20mm boxes, one rotated sub). → **5211** (high,
already claimed by the orchestrator).

## 14. Pattern-scale booleans: sieve floor is >10min even in release CLI (new)
Top-deck sieve, Ø3mm holes on 5mm hex pitch = 4,602 holes via two
linear_pattern_2d grids. Ladder: 33 holes ≈ 1s; 1,157 holes = 297s;
4,602 > 600s (full run pending at the time; re-measured 2026-07-21 — see
the addendum below: now completes in ~100s). GUI (debug build) = no
iteration at all.
Levers: single-pass BOPAlgo, manifold/OpenVDB kernel routing (already in
workspace!), native perforate op, progress+cancel. Bonus defect: pattern
direction/spacing args are BARE unitless numbers (mm implied) — violates
the always-units principle. → **5213** (high).

## Addendum — 4,602-hole sieve re-measurement after single-pass fuse (2026-07-21)

**Method.** Re-measured on the **release** CLI (`cargo build --release -p
reify-cli`) using the **flat single-grid perforating sieve**
(`examples/perforated_plate.ri`, scaled — added by 5213), NOT
`designs/litter_tray/top_deck.ri`: the latter's nested
`difference(difference(fillet…), union_all(pattern))` silently no-ops the
holes (byte-identical STEP output / mass across 33..4,602 holes) — a
by-product correctness bug found during this re-measurement and filed
separately as → **5318**. Each ladder rung is a scaled copy of the flat
sieve (two interleaved `linear_pattern_2d` grids, same 10mm pitch / Ø4mm
holes as the original, grid instance counts scaled to hit each rung);
holes are STEP-verified per rung via `grep -c CYLINDRICAL_SURFACE
out.step`, which lands EXACTLY on the hole count at every rung below —
proof the cut actually perforated, unlike top_deck.ri's no-op.

**Ladder (first-hand, this session).** `/usr/bin/time -v reify build
sieve_N.ri -o out.step`; wall-clock and CPU (user+sys) time both shown —
the host ran under heavy concurrent load during measurement (`uptime`
load average ≈114 on a 32-core box), which inflates wall-clock at the
smaller/faster rungs relative to CPU time:

| holes | wall-clock | CPU (user+sys) | STEP CYLINDRICAL_SURFACE | prior single-pass run (task 5317) |
|---|---|---|---|---|
| 33    | 1.38s   | 0.32s  | 33 (verified)    | 0.5s |
| 301   | 5.33s   | 2.07s  | 301 (verified)   | 1.8s (rung "300") |
| 1,157 | 28.48s  | 9.59s  | 1,157 (verified) | 9.6s |
| 4,589 | 100.38s | 99.92s | 4,589 (verified) | 121.6s (rung "4590") |

CPU time reproduces the prior single-pass run's figures closely at every
rung (0.32↔0.5, 2.07↔1.8, 9.59↔9.6, 99.92↔121.6) despite the wall-clock
noise from host contention — the largest rung runs long enough to
saturate a core and wall-clock converges to CPU time (100.38s vs 99.92s).
This first-hand re-measurement confirms the prior run's headline: **the
full ~4,600-hole sieve now completes in well under two minutes**, where
it previously never completed.

**vs. finding #14's pairwise baseline** (pre-5213, pairwise BOPAlgo):
33 holes ≈ 1s (unchanged at this tiny scale — fixed overhead dominates);
**1,157 holes: 297s → 9.59s CPU / 28.48s wall (≈31×/≈10× faster)**;
**4,602 holes: >600s, never completed → completes at 4,589 holes in
99.92s CPU / 100.38s wall.** 5213's single-pass n-ary fuse (Lever 1)
delivered the speedup finding #14 hoped for on the FUSE; see the
go/no-go verdict below for what it did and didn't fix end-to-end.

**Verdict.** Lever 1 (5213's single-pass n-ary fuse) fixed the pattern
**fuse** to ~linear, but the end-to-end `difference(base,
union_all(pattern))` **cut** stays **superlinear** in OCCT. This
session's own data show it directly: CPU time between the two largest
rungs (1,157→4,589 holes, 3.97× more holes) grew 9.59s→99.92s (10.4×
more time) — a local exponent ln(10.4)/ln(3.97) ≈ 1.7, consistent with
the ~O(N^1.8–2) characterization already on record (task 5317) and well
above linear (which would predict ~4×, not ~10×, time for 4× the holes;
the smaller rungs show a lower apparent exponent because fixed per-run
overhead, ~0.3s of process/parse startup, dominates at low N and
flattens the curve). ~100s (this session) / ~121.6s (prior run) at
~4,600 holes, compounded by superlinear growth as designs get denser, is
OUTSIDE interactive range for dense-pattern design work ⇒ **Lever 1
alone is INSUFFICIENT.**

**GO on the narrow Lever-2 slice** — route selector-free
`difference(base, union_all(pattern…))` to Manifold's already-linked
`batch_difference` (near-linear in tool count; bypasses both the
superlinear OCCT cut and the incomplete Manifold attribute/selector-
provenance substrate, since selector-free tool bodies hit Manifold's
benign `Discarded` no-op rather than needing 4263/4636) — filed as
→ **5317**.

**Lever 3 (native perforate/replicate-faces): DEFERRED/no-go for now.**
`batch_difference` is the lower-cost near-linear path for this workload;
revisit Lever 3 only if Lever 2 proves insufficient.

**Broad general-Manifold mesh-boolean production route: separate
deferred bookmark**, unaffected by this verdict — → **5220** (gated on
the narrow slice above plus the Manifold provenance substrate, 4263 /
4636).

**By-product correctness bug found during this re-measurement**
(independent of the timing question): top_deck.ri's holes silently
vanish via a nested `difference(difference(fillet…),
union_all(pattern))` no-op — → **5318**.

No new follow-up tasks were filed by this milestone — 5317, 5220 and
5318 all already existed before this addendum was written; this
addendum is the canonical doc record of the decision that authorized
them.

---
Split-joint design (agreed with Leo): shiplap-family → "pinwheel 45° scarf"
floor seam flipping direction at y=0 ⇒ IDENTICAL halves (print 2, rotate one
180°); walls/rim butt with glue gap; full-height exterior screw tabs land in
1mm registration recesses; one M3×8 per side into heat-set inserts in the
lower wall; PU/epoxy + interior sealant bead. Constraint system caught a
real error during authoring (insert pocket vs 6.4mm wall → widened ledge to
5mm, shortened insert to 4mm). Halves: 258×263mm footprint (fits U1 bed),
0.761 kg each, CLI-verified.

---

# Retest session, 2026-07-22 (dogfood round 2)

Fresh release CLI + dev GUI built at main 0408733da2 (all round-1 fix
commits ancestors, verified per-task). Retest of every landed fix from
the 07-14 session, plus an 11-probe boundary battery that overturned
5318's premise. New fix tasks filed this session: **5337, 5338, 5339**;
premise corrections pushed into **5214** (in-progress) and **5318**.

## Retest verdicts (round-1 fixes)

| Task | Verdict | Evidence |
|---|---|---|
| 5193/5209 watcher + identity | **PASS** | non-atomic `printf >`+`cat >>` write on the watched file: engine ends holding full content, no diagnostics, no stuck partial |
| 5194 GUI mass props | **PASS at its seam; two sibling seams broken** | via `open_file`: mass 1.40705 kg, centroid z 21.508 mm, MoI positive ✓. But argv-launch load AND watcher-reload leave mass/centroid/moi_principal Undef (capacity computes) → **5338** |
| 5198 display fixes | **PASS (GUI channel)** | `lower_wall 6.4`, `1270 kg·m^-3`, constraint pretty-print `material.density > 0 kg·m^-3` ✓. CLI eval channel deliberately untouched: still raw SI + full float noise (`0.0063999999999999994 m`) — scope decision, noted not re-filed. Near-zero values escape rounding (centroid x = `0.000000000000386564423998`) → **5339** |
| 5201 rounded_box | **LANDED** | `examples/litter_tray.ri` (the fix's own example adopts this design's footprint); not yet exercised in the tray rewrite — deferred until 5208 lands so corner+edge strategy can be chosen once |
| 5211 GUI segfault | **FAIL — phantom observable** | kernel IsNull guards ARE in the binary (106fe26c ancestor), yet `open_file bottom_deck_split.ri` still kills the GUI in ~6s, exit 139, 3/3 repro. gdb: **stack overflow in recursive `compile_expr`** (fault at entry of `compile_expr_guarded_with_expected`, expr.rs:1332, on `tokio-rt-worker`) — the file's 9-level inline geometry expression blows the 2MB tokio worker stack in the debug build; CLI is clean because it compiles on the 8MB main thread. 5211's OCCT hardening fixed an adjacent hazard; its stated observable was never verified e2e → **5337** (high) |
| 5213 single-pass fuse | **LANDED but see #17** | prior "timing ladder" measurements below were fusing *scattered* tools (units bug) — the flat-sieve 07-21 addendum (unit-ful spacings) remains the only valid pre-existing perf data |
| 5208 curated fillets | **still broken, unchanged** | same 6 errors, same stale message citing 4360/4358; retest note + repro-file fix appended to the task |
| 5195/5196/5197/5206 | still open as expected | 57-warning topology noise still present on healthy models; STEP chatter still pollutes eval stdout |

## 17. Bare pattern spacings are read as SI METRES — 5318's premise was wrong (probe battery)

An 11-probe matrix (base ∈ {flat, filleted, nested-diff, nested-diff+fillet,
let-bound, reassociated} × tool ∈ {single cylinder, pattern, union of 2
patterns} × spacing ∈ {bare, unit-ful}), each verified by exact mass
arithmetic AND per-hole `CYLINDRICAL_SURFACE` census in STEP:

- **Every** bare-spacing probe cuts exactly ONE hole per pattern
  (instances land 10 m apart; only instance[0,0] intersects). The flat
  "control" 5318 believed works fails identically.
- **Every** unit-ful probe cuts ALL holes — including the exact
  `difference(difference(fillet,fillet), union_all(patA,patB))` shape
  5318 blamed (13/13 holes, mass exact to ~1e-7 g).
- Committed as the two-file discriminator pair
  `probe_5318_bare_spacing.ri` / `probe_5318_unit_spacing.ri`
  (identical geometry, only spacings differ) — the bare-spacing arm has
  since been retired, see the task 5356 update below.

**Update (task 5356, after 5214 landed):** 5214 added an eval-layer gate
that REJECTS bare (dimensionless) spacing/offset/origin args to
`linear_pattern_2d`, `linear_pattern`, `arbitrary_pattern`, and `mirror` —
a bare spacing now produces a compile/eval `Error` diagnostic and the op
is dropped, instead of the silent SI-metres mis-cut described above.
`probe_5318_bare_spacing.ri` (which demonstrated the old silent 2/13-hole
behavior) has been retired: its regression intent is now covered by
`crates/reify-eval/tests/pattern_spacing_units_e2e.rs`
(`linear_pattern_2d_bare_spacing_drops_op_with_error`,
`linear_pattern_1d_bare_spacing_drops_op_dimensioned_builds`).
`probe_5318_unit_spacing.ri` (the unit-ful control) remains as a live
dogfood example.

Mechanism (all three seams verified in source): compiler passes spacing
untyped (geometry.rs:1768) → eval wraps raw Value, no dimension check
(geometry_ops.rs:2526) → kernel `extract_f64` = `as_f64()`
(reify-kernel-occt lib.rs:3369): `Real(10.0)` → **10 metres**;
`10mm` → 0.01. Silent at every layer — the canonical silent-default.

Consequences: **5214** premise corrected (was "interpreted as mm",
cosmetic; actually metres, correctness) and bumped to high — updated
mid-flight for its live claimant. **5318** rescoped: nested composition
innocent; residual scope = the boolean pipeline silently dropping
non-intersecting tools (partial cut at 9-tool scale, TOTAL silent no-op
at 4,602-tool scale — a "sieve" that builds hole-free with zero
diagnostics) + a build-path perforation regression test; now depends
on 5214. Old timing "ladders" measured fusing scattered tools, not
cutting holes.

## top_deck.ri fixed and verified (first true perforation)

Pattern calls now pass `nx_a/ny_a` + `hole_pitch`/`2*row_step`
(unit-ful lets, also exercising expression-valued pattern args — no
example covered that). Result: **mass 0.7929944333280107 kg**, matching
0.99724 (solid) − 0.20655 (4,602 Ø3×5mm holes) + 0.00215 (~48 holes
re-plugged by the two Ø26 pads) = 0.7928 kg — the design's pad logic
verified by arithmetic. Eval of the true 4,602-hole nested cut runs
**tens of minutes** (vs 100s for the flat sieve of the same hole count,
vs 12s for the bare no-op) — strengthens 5317's GO (Manifold
batch_difference); exact timing in the build log addendum when it
completes.

## 15. (→5338) Mass-props Undef on argv-launch and watcher-reload GUI load paths — see table row 5194.

## 16. (→5337) GUI SIGSEGV = compiler stack overflow on deep geometry exprs — see table row 5211.

## 18. Module-path enforcement vs design-variant workflow (intended behavior, real friction)

`E_MODULE_PATH_MISMATCH` (task 3977, spec §7.1/§7.2, recently extended
to the CLI entry path) now rejects `bottom_deck_v2_selectors.ri`
declaring `module bottom_deck` — the committed 5208 repro wouldn't even
reach its own bug. Fixed the declaration in-repo. Workflow note: keeping
side-by-side variants of one module (v1/v2/v2_selectors) now requires
renaming the module per variant — fine, but the v1↔v2 diff is noisier.
Also still present: `orient_identity()` in sub placement warns "cannot
infer return type of zero-arg function, defaulting to Real" on every
eval (finding #12's inference half; 5204 covered only the docs half).

## Session tooling notes

- GUI driven end-to-end over the debug MCP (health/open_file/
  engine_state/wait_for_idle/demand_dispatch) — this workflow held up
  well; `wait_for_idle` returning 28ms for a file that then crashes the
  process 6s later means idle ≠ settled for async tessellation/query
  passes (worth remembering when scripting retests).
- gdb -batch over the debug binary + MCP trigger = clean crash-stack
  capture recipe for GUI-native crashes.

## Round-2 completion numbers (post-addendum)

- Fixed top_deck STEP build: **4,616 CYLINDRICAL_SURFACE** (full hole
  field; pad-edge partials account for the delta vs 4,602), geometry+
  export **6:45 wall** (load ~150), peak RSS 674 MB. The ~30-min eval
  wall for the same file is dominated by the Rigid mass-prop kernel
  queries over the 4,616-face solid — mass-props on dense-perforated
  bodies are their own cost center (relevant to 5197/5317 framing).
- Bottom deck grew its mid-span pedestals (Ø20 at ±120, floor→ledge,
  matching the top deck's Ø26 pads): mass 1.45573 kg and capacity
  7.0067 L both match hand arithmetic exactly; constraints all green.
  Base-seam fillet deferred to curated selection (#5208 breadcrumb at
  the impl site).
- Next design steps: port pedestals + hand-slot interface into
  bottom_deck_split.ri (each half gets one pedestal, |x|=120 clears the
  x=0 seam), author the top-deck split (solid seam strip through the
  sieve field), then the fit-test print (one bottom half + sieve coupon
  to tune fit_clearance and hole size).

---

# Retest session, 2026-09-03 (dogfood round 3)

Binary `target/debug/reify` built at main b4766d4f0c (ancestors verified per task:
5337 d453a25c31, 5357 45335a28db, 5208 cfeb81976d, 5201 99ff5be70d, 5338
f93522dbbc, 5339 00500c223e, 5214 92551d18d6, 5195 d34278cffc, 5196
44d45aa4c8). Host under heavy load throughout (merge-queue verify; load 110–225).
Design rulings by Leo this round: hooded finger pockets on the bottom deck,
glue-only top-deck split with internal tabs, ledge_width synced to 5 mm, and a
full rewrite of all three decks to selector-driven fillets. Probes, fixtures and
the working ledger: `/home/leo/.claude/fleet/sessions/dogfood-reify-987649/`.

## Baselines re-measured (cold / warm / warm, values byte-identical across runs)

| file | mass | capacity | eval wall |
|---|---|---|---|
| bottom_deck.ri (round-2 file) | 1.455729562567982 kg (= table) | 7.0067 L (= table) | 3.55 / 3.71 / 3.52 s |
| bottom_deck_split.ri (round-2 file) | 0.7607200616 kg/half (= table) | — | 4.24 / 4.33 / 4.32 s |
| top_deck.ri (round-2 file) | 0.7929944333280107 kg (= table), 4,616 CYLINDRICAL_SURFACE (= table) | — | **eval 453 s (7:33)**, build 409 s (6:49), check 379 s |
| bottom_deck_v2_selectors.ri | 1.5142278520 kg (NEW — it evals) | 6.958 L | 1.33 s |

top_deck eval was "~30 min" on the round-2 binary; now 7.5 min with identical
output. 5317/5220 (Manifold batch_difference) are still pending, so this is not
Lever 2 — unattributed kernel/eval improvement, noted not filed.

## Retest verdicts (fixes landed since 2026-07-22)

| Task | Verdict | Evidence |
|---|---|---|
| 5214 bare spacings → eval error | **PASS** | `probe_5214_bare.ri`: exit 1, `error: linear_pattern_2d: spacing1 argument expects Length, got Int; pass a dimensioned length such as \`5mm\`` (one per axis). Unit-ful control: 10 holes cut, mass 8.784915959694686 g vs hand 8.784915959694688 g. Message names the argument, the type, and the fix — good UX. |
| 5208 curated fillets | **PASS (CLI) / FAIL (GUI)** | CLI: `bottom_deck_v2_selectors.ri` evals with 0 errors, all seven chained 3-arg fillets realized, 1.33 s (was six "curated edge selection is not yet available" errors on 07-22). GUI: the same file via `open_file` (and every other load path) fails with 10 compile errors — finding #22, → **#7256**. |
| 5201 rounded_box | **LANDED but does NOT compose with 5208** | finding #19 (→ #7054) |
| 5196 warning floor | **PASS (CLI + GUI)** | CLI: 0 Warning lines on healthy models; the topology chatter is ONE `info:` summary per realization (was 57 warnings). GUI: 0 Warning-severity topology entries on bottom_deck / the split (Info only). One Warning does remain on every Rigid part — the stale MoI ghost, finding #21. |
| 5195 hidden intermediates | **PASS** | probe_5195_vis.ri (plain `let a`, `let b`, `aux let c`, product `geometry`): mesh_stats lists all four, `viewport_state.meshCount` = 1 — only the DisplayOutput subject is drawn; plain and aux lets behave identically (hidden). Caveat: mesh_stats has no visibility flag (banked on #6752). |
| 5337/5357 GUI SIGSEGV on bottom_deck_split.ri | **PASS ×3** | round-2 `bottom_deck_split.ri` (the 9-level inline expression) opened three times with `bottom_deck.ri` between: settled in 11–14 s each, process alive 30 s after settle, both posed halves rendered (2,986 faces each, bboxes x ±[0.002, 0.260]), 0 compile diagnostics. Was exit 139, 3/3 on 07-22. |
| 5338/5339 mass-props load paths / near-zero rounding | **PASS** (argv launch, open_file, watcher reload) | GUI launched with `bottom_deck.ri` as argv: after settle `engine_state` shows mass 1.45572956257 kg, centroid `point(0, 0, 21.9428391363)`, full inertia tensor, `moi_principal` determined/final, the MoI constraint Satisfied (round 2: all undef). Near-zero centroid components render as `0`, `lower_wall` as `6.4`. The diagnostics list still shows the round-2 "indeterminate: undefined inputs: moi_principal" warning — a stale ghost, finding #21, not a 5338 regression. CLI channel still prints raw near-zero noise — out of 5339's scope by the round-2 ruling. |
| 5197 / 5206 / 5202 / 5318 / 5551 | still open, as expected | instance-scope `note: … (OpContractViolation)` unchanged; OCCT STEP banner + `Step File Name : /tmp/reify_step_…` still on every eval. |

## 19. `rounded_box` is a never-unified 6-body fuse — curated height-selected fillets on it fail (→ combined into #7054)

The 5201 primitive (built for this tray) and the 5208 capability (just made
designer-reachable) do not compose. STEP census: a bare `rounded_box(100,60,20,10)`
exports **50 faces (46 planar) and 88 edges** where the exact prism has 10 and 24;
identical mass to 1e-12. The compiler desugars `rounded_box` to 2 boxes + 4
cylinders left-folded through five `Union`s (geometry.rs:2707 → :1279); the kernel's
`boolean_fuse` returns raw `BRepAlgoAPI_Fuse` output and the workspace contains no
`UnifySameDomain`/`ShapeUpgrade`/`ShapeFix` call at all; `edges_at_height`
(topology_selectors.rs:902) is a pure bbox-z predicate over ALL edges, so it feeds
the coplanar seam edges lying in the top face to `BRepFilletAPI_MakeFillet`, which
aborts: `OCCT make_fillet_edges_with_history: unexpected: BRepFilletAPI_MakeFillet
failed (curated per-edge selection)`, followed by the #5208-signature cascade
(`unresolvable GeomRef::Sub('rim_soft')`…) and `error: all realized bodies are aux;
no product geometry to export` from plain eval. Bisected on the committed v2 file:
swapping only its three blanks to `rounded_box` → FAIL; `aux let`, a self-union, or
unioning pedestals/hoods before the chain → PASS. Intermittent from the designer's
seat (which seams land at the selected height decides). Side effect: a coupon on
`rounded_box` exported 104 KB / 21 planes vs 54 KB / 9 planes on the box +
`edges_parallel_to` blank, and evaluated 2× slower. Workaround adopted in every
rewritten file: box blank + `edges_parallel_to(blank, +Z, 1deg)` corner fillet.
Curator folded the filing into **#7054** (third symptom of "boolean output is never
normalised", with the bare-COMPOUND containment blindness and #6402's BRepKind stamp).

## 20. Rigid sub-instance: `geometry` cell undef at instance scope, MoI undef, mass served unposed (→ amended into #7065)

Minimal fixture `probe_sub_massprops.ri` (20 mm brick, two placed subs): template
scope all defined; `Pair.a.mass` served, `Pair.a.centroid` served but UNPOSED
(#6583), `Pair.a.geometry = undef`, `moment_of_inertia`/`moi_principal` undef with
the #5197 string, and the instance snapshot claims even `mass` is undef. Same
signature on both split assemblies. Third carrier for #7065 (with #6662/#7184 owning
the instance-scope undef class); recorded there, not filed.

## Round-3 design results (all files rewritten to the selector idiom; every mass hand-checked)

| file | what changed | mass | check |
|---|---|---|---|
| bottom_deck.ri v3 | box+`edges_parallel_to` blanks, `edges_at_height` rim/ledge/floor/base chain, ledge 5 mm, Ø20 pedestals at ±120 (base seams caught by the floor fillet), hooded finger pockets in both end walls (60×22 slot at z 36..58, 70×25×57 hood, roof top z 61) | **1.5847031 kg**, capacity **6.7203 L** (hand: 1.5856 kg, Δ 0.9 g of fillet-fill approximation; capacity exact) | green, 1.8 s |
| bottom_deck_split.ri v2 | same body + one pedestal + one hooded pocket per half, fillets BEFORE the seam trims (glue faces stay sharp), scarf cutters parametric in strip_half, declarative `STEPOutput` → bottom_half.step | **0.7955804 kg/half** (hand 0.7956648 from monolithic/2 + tab − recess − insert: Δ 0.09 g) | 23/23 green, 3.1 s; bottom_half.step = 1 solid |
| top_deck.ri v2 | selector shell + rim 1 mm / floor 4 mm / base 1.5 mm fillets BEFORE the punch, then the two grids, pads, slots | **0.7930940 kg**, 4,602 holes (round-2 0.7929944; Δ +0.10 g = the fillet-scheme change, hand +0.1 g) | eval 1,154 s / check 955 s / build 1,010 s (sequential, load ~100–130) — **2.5× the round-2 file at every leg**, finding #23; STEP 1 solid, 4,620 cylinders |
| top_deck_split.ri v1 (NEW) | glue-only pinwheel scarf through a solid seam strip, first columns x 6.25 / 8.75 (1.25-mod-5 phase ⇒ one continuous hex lattice across the seam after the 180° rotation), 44 + 43 columns × 26 rows = **2,262 holes/half** (4,524/tray), one pad + one hand slot per half, chamfered internal overlap tab 36×2.4×30 at z 9.5..39.5 on the +y inner wall (clears the mate's 4 mm floor fillet; 45° underside prints support-free), `STEPOutput` → top_half.step | **0.4010616 kg/half** (hand 0.3982 + tab 2.8 g = 0.4010) | eval 450 s / check 427 s / build 567 s; STEP of the assembly: 2 solids, 4,542 cylinders; `top_half.step` via the declarative output |
| sieve_coupon.ri (NEW) | 60×60×5, Ø3/5 mm hex, 85 holes, `hole_d` swept | 19.044732071756062 g (hand Δ 1.4e-14 g) | green; 85 cylinders in STEP |
| fit_coupon.ri (NEW) | +x+y corner of both decks (bottom z 50..80 with ledge + recess; top z 0..25), `fit_clearance` swept, `FitCheck.penetration = volume(intersection) = 0` exact | 19.43752388617921 g / 29.199596954040036 g (hand Δ 5e-14 / 7e-15) | green; per-structure STEPOutput |

Bed check per bottom half unchanged (258 × 263 mm); top half 239.7 × 254.4 mm.

## 21. Ghost "indeterminate" warning on every Rigid part (→ amended into #6979)

After the mass-property post-process fills `moi_principal`, the constraint panel
reports the MoI constraint Satisfied and the cell is determined/final, yet the
diagnostics list keeps the first-pass `constraint … indeterminate: undefined
inputs: …moi_principal` Warning. #6979's Indeterminate re-check loop flips the
status but discards its diagnostics, so the Warning is never retracted. It is the
round-2 5338 "evidence" signature and it briefly fooled this round's argv-launch
retest. Consequence: a permanent floor of one Warning per Rigid body.

## 22. GUI cannot resolve curated topology selectors at all — 5208's GUI half never landed (→ #7256, high)

`open_file` on the committed 5208 repro (`bottom_deck_v2_selectors.ri`, plain
lets) settles with 10 compile errors: four `fillet(solid, edges, radius): the
edge selector did not resolve to a concrete edge list at the point this fillet
runs … [edge selector evaluated to Undef]` plus the `unresolvable GeomRef::Step`
/ `GeomRef::Sub('hollowed'…)` cascade; `demand_dispatch` shows only the three
blank boxes dispatched and the viewport draws only those. Identical on the
watcher-reload path and with `aux let` intermediates (19 errors on the v3
bottom deck, 33 on the top-deck split). The CLI evaluates every one of these
files clean on the same binary. What 5208 did deliver in the GUI is loudness
(round 1 was silent). Consequence: every selector-driven design — the whole
round-3 rewrite — renders nothing in the GUI until #7256 lands. Leo's ruling:
keep the rewrite; the fit-test print goes from the CLI STEP exports; GUI
screenshots this round come from the round-2 files.

## 23. Fillet-first construction costs 2.5× on the dense boolean (observation → data point on #5317)

The v2 top deck (selector fillets on the shell, then the 4,602-hole punch) takes
1,154 s to eval, 955 s to check and 1,010 s to build, against 453 / 379 / 409 s
for the round-2 file under heavier load, with identical mass and hole census.
OCCT's cut of 4,602 cylinders against a shell carrying toroidal fillet faces is
~2.5× the cut against all-edge-filleted primitives. The order is forced: a height
selector at z = floor or z = 0 after the punch would select every hole rim.

## 24. The GUI wedges on the 4,602-hole deck and starves its own debug bridge (→ amended into #6752)

`open_file` on the round-2 `top_deck.ri` exceeded the MCP tool timeout; one
engine thread then ran at 80–100% for over an hour (35 CPU-min at the 34-minute
mark, RSS cycling 0.8→3.9 GB) while `mesh_stats`, `demand_dispatch`,
`store_state` and `wait_for_idle` blocked and `get_diagnostics` kept returning
the previous file's diagnostics; by the hour mark even `health` stopped
answering within 8 s. No cancel exists. The "poll mesh_stats until stable"
settle recipe cannot tell tessellating from bridge-blocked. Two smaller GUI
findings from the same battery: the viewport's minimum orbit distance is clamped
at 0.5 m, so parts under ~150 mm cannot be framed (→ combined into #6965), and
`mesh_stats` carries no visibility flag (on #6752).

## Session tooling notes (round 3)

- The CLI recipe `export LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu:/opt/reify-deps/lib`
  must NEVER be in the shell that runs `scripts/run-gui-dev.sh`: the launcher prepends
  the snap OCCT dir to the caller's path, and with the system dir first the system
  `libtbb.so.12.11` shadows the RUNPATH `tbb-pin` (deps 12.18), so the GUI dies at exec
  with `symbol lookup error: undefined symbol: _ZN3tbb6detail2r127get_thread_reference_vertex…`
  (task 5192's mechanism A″ is defeated by LD_LIBRARY_PATH, which beats DT_RUNPATH).
  Launch with `env -u LD_LIBRARY_PATH WEBKIT_DISABLE_DMABUF_RENDERER=1 REIFY_DEBUG_PORT=<port>
  bash scripts/run-gui-dev.sh …` — the second variable is required on this NVIDIA host
  (Mesa's EGL cannot initialise the GBM device: `Could not create GBM EGL display:
  EGL_NOT_INITIALIZED`, a core dump), which earlier sessions knew and no doc stated.
  This cost the post-landing GUI relaunch ~30 min this round. → **#7254** (filed by the printer session; scrub/pin + orphan-vite teardown).
- `reify check` on a kernel-backed module = a full `build()` + a re-run `check()` (done
  #5748), so it is 4× `eval` on an 85-hole coupon and 6.3 min on the 4,602-hole deck.
  Iterate with `eval`; gate with `check`.
- `reify build <file> -o x.step` writes EVERY product body reachable in the file into one
  STEP (both posed halves of a split assembly, 2 solids) with no notice; for one named
  file per structure use a declarative `sub step_out = STEPOutput(subject: geometry,
  path: "…")` and `reify build <file> --out-dir <dir>` (evidence banked on #6648).
- Cross-sub geometry reads come back UNPOSED (#6583, re-confirmed by the fit coupon):
  the mated-position check lifts the top body by hand and gates on
  `volume(intersection(a, b))` — the distance family stays containment-blind on
  boolean bodies (#7054).
- Curator combines can CLOBBER a previous repair: the #7054 combine dropped the O5
  session's restored symptom-2 evidence and paraphrased symptom 3 into a wrong mechanism;
  always `get_task` after `resolve_ticket` and rewrite the whole record if needed.
- A fused-memory `update_task` that times out may or may not have landed (the #6648
  amendment did NOT; the #7065 one did) — read back before resending.
