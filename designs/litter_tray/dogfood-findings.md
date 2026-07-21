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
