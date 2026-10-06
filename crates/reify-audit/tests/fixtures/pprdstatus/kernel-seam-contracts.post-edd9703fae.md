# Kernel-Seam Contracts and Conformance

**Status: SHIPPED.** All 16 decomposition leaves have landed — α #5102, β #5103, γ #5104, δ #5105,
ε #5106, ζ #5107, η #5108, θ #5109, ι #5110, κ #5111, λ #5112, μ2 #5113, μ3 #5114, ν #5115, ξ #5116,
plus the adopted #4876. All four **INV-GEO-1..4** rows in `docs/invariants.md` carry an
`enforced(...)` status column; note that INV-GEO-2 is co-owned, and its row still annotates the
`KernelHandle`-keyed-table half (#4351, engine-build-hardening) separately from the
conformance-property-test half that this PRD delivered. Authored 2026-07-06 in an interactive `/prd` session as part of the bug-hotspot
program (`docs/notes/bug-hotspot-survey-2026-07-05.md` §H3); B+H full shape. Owner PRD named in
`docs/invariants.md` for those four IDs. Shipped-status recorded 2026-08-19.

Adopt existing task **#4876** (`deferred`, high) — do not duplicate. Decomposed into three leaves:
