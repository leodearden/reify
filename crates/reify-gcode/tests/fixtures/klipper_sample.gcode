; reify-gcode Klipper-dialect round-trip fixture — exercises every
; Klipper command kind the parser supports (PRD
; docs/prds/v0_3/trajectory-input-shaping.md §7.1 + §11 task ν).
; Comments, blank lines, and indented lines are intentional.

SET_VELOCITY_LIMIT VELOCITY=200 ACCEL=3000
SET_VELOCITY_LIMIT ACCEL=2000 VELOCITY=150 SQUARE_CORNER_VELOCITY=5
SET_VELOCITY_LIMIT

INPUT_SHAPER SHAPER_TYPE=ei SHAPER_FREQ_X=40 SHAPER_FREQ_Y=42
INPUT_SHAPER

M82                ; absolute extruder mode
M104 S200          ; set extruder temp without waiting
M109 S210          ; set extruder temp and wait

G92 X0 Y0 Z0 E0    ; reset all axes
G0 X5 Y5 Z0.2      ; rapid to start
F1500              ; standalone feedrate

G1 X10 Y5 E0.5
   G1 X10 Y10 E1.0
G1 X5 Y10 E1.5 F1200
G1 E0.5

G2 X0 Y10 I-2.5 J0 F800
G3 X-5 Y5 I0 J-5

G1 Z1.0 F600
M83
G1 E-0.5

G92 E0             ; final reset comment
