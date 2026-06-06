; printer_print_envelope.gcode — bolt-on G-code fixture (task ρ — 3878)
; Two motion runs separated by one M104 temperature command.
; gcode_import under MarlinDialect lowers this to >= 2 contiguous motion segments.
; Only commands supported by the Marlin parser are used: G1 (LinearMove),
; M104 (IgnoredMCode = segment splitter). G28 is not supported and would
; cause a ParseError, so homing is omitted.
G1 X50 Y10 F3000    ; approach: first motion run start
G1 X150 Y10 F3000   ; print stroke end
M104 S200           ; set hotend temperature (IgnoredMCode — segment splitter)
G1 X150 Y20 F3000   ; second motion run start
G1 X50 Y20 F3000    ; return stroke end
