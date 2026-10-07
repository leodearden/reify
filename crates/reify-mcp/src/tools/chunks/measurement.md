# Measurement & Mass-Property Queries
<!-- MEASUREMENT-SECTION -->

<!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks/measurement_chunk_smoke.rs verifies that
     this section documents EVERY member of reify_compiler::GEOMETRY_QUERY_NAMES as a call form.
     That test iterates the registry DIRECTLY rather than a list copied into the test, so a query
     added to the compiler and not to this section is RED on the next run — there is no second
     list to forget.

     FORMAT IS LOAD-BEARING in one place only: for `volume` / `area` / `centroid` /
     `bounding_box`, the literal `-> <Type>` after the call form is what marks the form as a
     SIGNATURE whose arity is cross-checked against the worked fence. Tabulating those four into
     a table with the return type in its own column is RED even though nothing regressed.
     Everything else here — bolding, wrapping, ordering, the heading's wording — is free.

     Chunk-side guards (all in that one file, cited whole on one line each):
       measurement_chunk_smoke.rs::measurement_query_family_documented_in_measurement_chunk
       measurement_chunk_smoke.rs::measurement_reify_fences_call_every_whole_handle_query
       measurement_chunk_smoke.rs::measurement_signature_arities_match_the_compiling_fences
       measurement_chunk_smoke.rs::the_undef_trap_example_is_a_query_the_hoist_does_not_cover

     That last guard scopes the region BETWEEN the `NOT-HOISTED-TRAP` and
     `/NOT-HOISTED-TRAP` markers below — both matched byte-exactly, and a missing
     closing one is RED rather than a silent widening — and pins exactly ONE
     claim: the call form the arg-shape trap exhibits is drawn from OUTSIDE
     reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES, so the trap cannot illustrate "an inline
     argument yields undef" with one of the four names the next sentence exempts. It reads the
     registry directly, so a name entering or leaving the hoisted set moves it. What it does NOT
     pin: the traps' wording, the binder-scope claim in trap 2, and the `undef` behaviour itself —
     all UNPINNED prose here. Trap 2 is pinned on the compiler side instead, by
     crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs::assert_hoist_scope
     whose three binder cases each assert exactly zero hoists. Naming a whole-handle query as a
     bare backticked identifier in the carve-out sentence is fine — only the CALL FORM is refused.
     The closing marker sits directly after the third trap, so the **Worked reference** paragraph
     that ends this section is OUTSIDE the guarded region and may name any call form it likes.

     RUNTIME claims below (which names resolve to real numbers, and when they do not) are pinned
     separately, on the eval side:
       crates/reify-eval/tests/harness_kernel_realization/kernel_queries_integration.rs::all_queries_walk_evals_top_level_helpers_to_non_undef
       crates/reify-eval/tests/harness_kernel_realization/kernel_queries_integration.rs::multi_feature_part_sub_handle_queries_return_non_undef
       crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs::whole_handle_geometry_query_oracle_parity
       crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs::compile_inline_volume_torus_hoists_into_realization
     The OCCT-absence claim in "When a query yields `undef`" is UNPINNED prose — verified by
     reading the gate in crates/reify-kernel-occt/src/lib.rs, not by a test in this harness.

     The `MEASUREMENT-SECTION` marker on the line above is what scopes the guard's scan,
     matched byte-exactly — NOT this heading's wording. Keep it directly under the heading it
     opens; the scan runs from it to the next `##` heading. -->

Reify measures **realized geometry**. Never hand-compute a volume, an area, a centroid or an
inertia from the parameters that built the part — ask the kernel. Parameter arithmetic
(`thickness * width * width`) is a *different number*: it silently stops describing the part the
moment a fillet, a shell, a boolean or a pattern changes it, and nothing flags the divergence. The
family below is the supported way to ask.

**Measurement.** `volume(solid) -> Scalar<Volume>`, `area(surface) -> Scalar<Area>` (also
`area(solid) -> Scalar<Area>`, total surface area), `length(curve) -> Scalar<Length>`,
`perimeter(surface) -> Scalar<Length>`.

**Mass properties.** `centroid(solid) -> Point3<Length>` is the purely geometric centre — no
density argument, no material. `bounding_box(geometry) -> BoundingBox` is the axis-aligned extent.
For the *density-weighted* pair, `center_of_mass(solid, density)` and
`moment_of_inertia(solid, density)`, see the `topology` chunk: they are registered in
the topology-selector family, not this one, and both take a **dimensioned** `Density` (a bare
number yields a warning and `undef`).

**Predicates.** `contains(solid, point) -> Bool`, `intersects(a, b) -> Bool`,
`geo_equiv(a, b, tolerance) -> Bool` (tolerance must carry a Length unit).

> `contains(solid, point)` is the free-function GEOMETRY query documented here. It is a different
> thing from the `contains` **method** on `List` / `Set` / `Range` in the `collections` chunk — same
> word, unrelated dispatch. Reaching for the collection method on a solid, or this query on a list,
> gets you a type error at best and `undef` at worst.

**Analysis.** `distance(a, b) -> Scalar<Length>` (true minimum surface gap — the `geometry`
chunk's interference/clearance section owns it), `normal(surface, at) -> Vector3<Dimensionless>`,
`curvature(curve, at) -> Scalar<Curvature>` (the surface overload is
`curvature(surface, at) -> Matrix<2, 2, Curvature>`), `angle(a, b) -> Angle` — note `angle` takes two
dimensionless **vectors**, not two surfaces; the surface form is `angle_between_surfaces` in the
topology-selector family. `max_deviation(actual, nominal) -> Scalar<Length>` compares a realized
geometry against a nominal one.

**Provenance.** `feature(geometry) -> Feature` is the explicit projection from a realized handle to
the feature that produced it. It returns a `Feature`, not selectable geometry — it is the *input* to
the provenance selectors (`created_by_feature`, `split_by_feature`) in the `topology` chunk.

```reify
structure def MeasuredBracket {
    // Let-bind the geometry FIRST. That is the arg-shape contract, not style.
    let plate = box(60mm, 40mm, 8mm)

    // Ask the kernel. Never re-derive these from 60mm * 40mm * 8mm: the moment a
    // fillet or a pocket lands, the arithmetic silently stops describing the part
    // and the queries keep up.
    let v = volume(plate)
    let a = area(plate)
    let c = centroid(plate)
    let bb = bounding_box(plate)

    // These four whole-handle queries are the only ones that ALSO accept an
    // inline geometry argument -- `volume(box(60mm, 40mm, 8mm))` works, because
    // the compiler hoists the inline call into a synthetic let for you. Every
    // other query in the family needs the let-bound form above, so writing
    // let-bound everywhere is the one rule that never bites.

    // A measured scalar driving a real gate, not a dangling let.
    constraint v < 25000mm^3
}
```

## Eval status, and when a query yields `undef`

Every one of these fifteen names has live eval dispatch. There is **no** "compile-time typed but
never evaluated" subset in this family. What there is, is a resolution *stage*: these are
kernel-bearing consumers, resolved against a realized handle. `reify build`, `reify eval` and
`reify check` all realize geometry for a module that carries some, so all three resolve them. On a
**kernel-less** surface — a build without OCCT, or an in-process `Engine::new(.., None)` — they stay
`Value::Undef` instead, and a constraint over one reads `INDETERMINATE` while the process still
exits 0 (`--strict` flips that). This is the same stage split the `geometry` chunk's
interference/clearance section documents at length.

Three things make a query silently `undef` even under `reify build`, and all three are worth knowing
before you debug the number:

<!-- NOT-HOISTED-TRAP -->
1. **Arg shape.** A geometry operand must be a **let-bound** reference. An inline call
   (`perimeter(rectangle(40mm, 20mm))`) is not resolved against the named-step map and yields
   `undef` with no diagnostic. The one carve-out is the four whole-handle queries — `volume`,
   `area`, `centroid`, `bounding_box` — where the compiler hoists an inline geometry argument into
   a synthetic let for you. Every other query in the family requires the let-bound form, so write
   let-bound everywhere and the rule never bites.
2. **Binder scope — where that carve-out stops.** The hoist is a post-process over structure MEMBER
   value cells, and it treats a lambda, a quantifier and a `match` as **opaque**: it neither
   descends into one nor rewrites inside one, because the argument is lifted verbatim out to the
   member list, where a name bound by the lambda param / quantifier variable / match binder would
   be unbound. So the carve-out simply does not apply under a binder — a quantifier predicate
   applying `volume` to an inline `box(s, s, s)` keeps the base `undef` behaviour, whole-handle or
   not. The guard is structural rather than a free-identifier analysis, so this is conservative in
   both directions: an inline query that happens to use no bound name is left un-hoisted too.
   Remedy: let-bind the geometry at member level. When the geometry depends on the bound variable
   itself there is no let-bound rewrite to reach for, and that cell stays `undef` today.
3. **No OCCT.** The kernel gate is all-or-nothing, not per-query: with OCCT unavailable the whole
   geometry pipeline is skipped, every query cell stays `undef`, and the exit code is still 0. There
   is no per-query fallback to a cheaper representation, so do not write code that expects one.
<!-- /NOT-HOISTED-TRAP -->

**Worked reference:** `examples/kernel_queries/all_queries_walk.ri` calls the family end-to-end over
a multi-feature part. Read it as a runnable example, not as normative prose — its own header is
partly stale about selector result types. The normative signature tables are
`docs/reify-stdlib-reference.md` §3.9 (`std.geometry.query`), which this section deliberately does
not restate. `max_deviation` is the one member §3.9 does not yet list; its signature above comes
from the compiler registry in `crates/reify-compiler/src/units.rs`.
