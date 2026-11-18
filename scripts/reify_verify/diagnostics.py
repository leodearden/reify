"""diagnostics — the prefixes that mark a stderr line as verify.sh's own warning or error,
or as the error of a lib verify.sh sources.

It imports nothing from the package, so every module that prints a diagnostic
can import it. The `verify.sh: WARNING: invalid ...` lines in knobs and retry
keep bash's colon form byte for byte and do not use WARNING.
"""


def error_from(script: str) -> str:
    """The prefix `script` prints before an error."""
    return f"{script}: ERROR — "


WARNING = "verify.sh: WARNING — "
ERROR = error_from("verify.sh")
