"""diagnostics — the prefixes that mark a stderr line as verify.sh's own warning or error.

It imports nothing from the package, so every module that prints a diagnostic
can import it. The `verify.sh: WARNING: invalid ...` lines in knobs and retry
keep bash's colon form byte for byte and do not use WARNING.
"""

WARNING = "verify.sh: WARNING — "
ERROR = "verify.sh: ERROR — "
