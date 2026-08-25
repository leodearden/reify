# printer_v01 design-verification tools

Analysis scripts, NOT part of any build. They exist because the rear tendon
routing is dense enough that eyeballing the viewport is not a sufficient check
— the 2026-07 hand-verified layout shipped nine interfering pairs, and the
2026-08 rewrite of it introduced a wrap-sense error that a human caught by eye
after the scripts had passed it.

| script | question it answers |
|---|---|
| `v2_check.py` | Does any pair of solids in the rear zone interfere? Sweeps all pairs of the ~84 rear-zone solids (ropes, pulley discs, drum, collar) and reports clearance, suppressing by-design contacts explicitly rather than silently. |
| `incidence.py` | Does every pulley have exactly TWO tendon segments on it (one in, one out)? Reads `v2_check.py`'s solid table. This is what caught 18 tendon segments failing to realize. |
| `front_check.py` | The same incidence question for the carriage idlers and front corners, which `v2_check.py` does not model. |

They mirror the DERIVATIONS in `printer.ri`'s `DriveTendons`, not its numbers,
so a disagreement means the script and the design disagree about a tangency
rule — which is the point. When `printer.ri` moves, update the constants block
at the top of `v2_check.py` and re-run.

KNOWN LIMITS, so nobody mistakes a pass for a proof:
- Tangency is necessary but NOT sufficient: it does not encode which way a rope
  wraps a pulley, so a pulley placed on the wrong side of its own rope still
  reads as correct here. `printer.ri` constrains wrap sense directly for that
  reason.
- Everything is evaluated at the fairlead's NOMINAL band position. Clearances
  across the migration stroke are constrained in `printer.ri` instead; a proper
  kinematic sweep study is the rigorous version and does not exist yet.
- Wrap arcs are not modelled anywhere, so rope length is not computable.
