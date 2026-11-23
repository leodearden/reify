"""gatepaths — the repo files more than one phase group's steps name, each declared once (PRD D12).

The lint wave and the test region both run verify.sh's compile gate, and both
guard a reify-gui cargo pass on the gui crate's manifest with its sidecar
placeholder first. The infra pool runs run_all.sh, and the retry marker asks
the same script for its count. Each emitter still declares the gate paths its
steps name, and these are the values those declarations read.
"""

from dataclasses import dataclass

from reify_verify.bashcall import DataFile
from reify_verify.plan import Chain, Command, Guarded, Script

VERIFY_SH = "scripts/verify.sh"
COMPILE_GATE = Script(VERIFY_SH, ("compile-gate",))
RUN_ALL = DataFile("tests/infra/run_all.sh")


@dataclass(frozen=True)
class CrateGuard:
    """A pass that runs only when its crate's manifest exists, after `setup`."""

    manifest: str
    setup: Script

    def around(self, body: Command) -> Guarded:
        """`if test -f <manifest>; then <setup> && <body>; fi`."""
        return Guarded("-f", self.manifest, Chain((self.setup, body)))


GUI_CRATE_GUARD = CrateGuard(
    "gui/src-tauri/Cargo.toml", Script("scripts/ensure-gui-sidecar-placeholder.sh")
)
