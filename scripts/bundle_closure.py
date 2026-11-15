#!/usr/bin/env python3
"""bundle_closure.py — derive the shared libraries the Reify installer must ship.

docs/prds/alpha-spine.md §5.2: the .deb and AppImage carry "the full runtime
shared-library closure of both binaries that is not in a stock Ubuntu 24.04
install (derived at build time from ldd, never a hand list)". This tool derives
that closure EMPIRICALLY, against the target rather than from host metadata:

  * host `ldd` (LD_LIBRARY_PATH removed) maps every soname the shipped ELFs
    need to the host file that satisfies it;
  * a reference image — ubuntu:24.04 plus the deb's own declared Depends, read
    from tauri.conf.json, plus patchelf — stands in for the installed target;
  * a fixed point stages the shipped ELFs at <stage>/usr/bin and each soname
    the target cannot resolve, or resolves short of a required symbol version,
    at <stage>/usr/lib/<productName>/lib, sets their RUNPATHs (patchelf, run in
    the reference image as the caller's uid), and re-runs `ldd` there on the
    as-installed layout until it reports nothing missing.

The staged tree is then what the .deb ships, through the tauri config overlay
this tool writes (`cargo tauri bundle --config <overlay>`). The AppImage gets
the UNPATCHED extra binaries instead: linuxdeploy deploys their dependencies
itself, by the binary's own RUNPATH.

Contract with scripts/build-bundle.sh: stdout stays empty; every diagnostic goes
to stderr; exit 0 means the overlay was written and the staged layout resolves
in the reference image.

Usage:
  bundle_closure.py --tauri-conf gui/src-tauri/tauri.conf.json --stage DIR \\
      --overlay OUT.json --main-binary reify-gui=PATH --extra-binary reify=PATH
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType
from typing import Callable, Mapping

BASE_IMAGE = "ubuntu:24.04"
IMAGE_REPOSITORY = "reify-bundle-ref"
LIBRARY_RUNPATH = "$ORIGIN"


class ClosureError(RuntimeError):
    """The closure cannot be derived: a library no host file supplies, or a
    round that staged nothing new while the target still reports a gap."""


class CommandError(RuntimeError):
    """A subprocess the derivation depends on exited non-zero."""


@dataclass(frozen=True)
class LddReport:
    resolved: Mapping[str, str]
    missing: frozenset[str]
    version_short: frozenset[str]

    def __post_init__(self):
        object.__setattr__(self, "resolved", MappingProxyType(dict(self.resolved)))
        object.__setattr__(self, "missing", frozenset(self.missing))
        object.__setattr__(self, "version_short", frozenset(self.version_short))


_RESOLVED = re.compile(r"^(?P<soname>\S+) => (?P<path>/\S+) \(0x[0-9a-f]+\)$")
_MISSING = re.compile(r"^(?P<soname>\S+) => not found$")
_UNMAPPED = re.compile(r"^\S+ \(0x[0-9a-f]+\)$")
_VERSION_SHORT = re.compile(
    r"^.+: (?P<path>/\S+): version `[^']+' not found \(required by .+\)$")
_NO_VERSION_INFO = re.compile(
    r"^.+: /\S+: no version information available \(required by .+\)$")


def _is_file_header(raw_line: str) -> bool:
    return not raw_line[:1].isspace() and raw_line.endswith(":")


def parse_ldd(text: str) -> LddReport:
    """Parse glibc ldd output, for one file or several. An unrecognised line is
    an error, so a changed ldd format fails loudly rather than reading empty."""
    resolved: dict[str, str] = {}
    missing: set[str] = set()
    version_short: set[str] = set()
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or _NO_VERSION_INFO.match(line) or _UNMAPPED.match(line):
            continue
        if match := _VERSION_SHORT.match(line):
            version_short.add(os.path.basename(match["path"]))
        elif _is_file_header(raw_line):
            continue
        elif match := _RESOLVED.match(line):
            resolved.setdefault(match["soname"], match["path"])
        elif match := _MISSING.match(line):
            missing.add(match["soname"])
        else:
            raise ValueError(f"unrecognised ldd line: {raw_line!r}")
    return LddReport(resolved=resolved, missing=frozenset(missing),
                     version_short=frozenset(version_short))


Probe = Callable[[frozenset], LddReport]


def derive_closure(host_resolved: Mapping[str, str], probe: Probe) -> frozenset[str]:
    """Stage sonames until the target reports none missing or version-short.

    probe(staged) stages exactly `staged` on the target and returns its ldd
    report; host_resolved says which sonames the host can supply."""
    staged: frozenset[str] = frozenset()
    while True:
        report = probe(staged)
        gap = report.missing | report.version_short
        if not gap:
            return staged
        unsupplied = gap - set(host_resolved)
        if unsupplied:
            raise ClosureError(
                "the target lacks libraries the host does not resolve either: "
                + ", ".join(sorted(unsupplied)))
        new = gap - staged
        if not new:
            raise ClosureError(
                "staging made no progress; the target still reports missing or "
                "version-short: " + ", ".join(sorted(gap)))
        staged = staged | new


def installed_lib_dir(product_name: str) -> str:
    return f"/usr/lib/{product_name}/lib"


def executable_runpath(product_name: str) -> str:
    return f"$ORIGIN/../lib/{product_name}/lib"


def staged_binary(stage: Path, name: str) -> Path:
    return stage / "usr" / "bin" / name


def staged_lib_dir(stage: Path, product_name: str) -> Path:
    return stage / installed_lib_dir(product_name).lstrip("/")


def tauri_overlay(stage: Path, product_name: str, main_binary: tuple[str, Path],
                  extra_binaries: Mapping[str, Path]) -> dict:
    """The tauri config merged over tauri.conf.json at bundle time: the deb
    ships the staged (RUNPATH-patched) tree, the AppImage only the unpatched
    extra binaries, since tauri already places its main binary there."""
    main_name, _ = main_binary
    deb_files = {f"/usr/bin/{name}": str(staged_binary(stage, name))
                 for name in [main_name, *extra_binaries]}
    deb_files[installed_lib_dir(product_name)] = str(staged_lib_dir(stage, product_name))
    appimage_files = {f"/usr/bin/{name}": str(path) for name, path in extra_binaries.items()}
    return {"bundle": {"linux": {"deb": {"files": deb_files},
                                 "appimage": {"files": appimage_files}}}}


def reference_dockerfile(depends: list[str]) -> str:
    packages = " ".join(shlex.quote(p) for p in [*depends, "patchelf"])
    return (
        "# Reference target for scripts/bundle_closure.py: stock Ubuntu 24.04 plus\n"
        "# the deb's declared Depends (tauri.conf.json), and patchelf to stage with.\n"
        f"FROM {BASE_IMAGE}\n"
        "ENV DEBIAN_FRONTEND=noninteractive\n"
        f"RUN apt-get update && apt-get install -y --no-install-recommends {packages}"
        " && rm -rf /var/lib/apt/lists/*\n")


def reference_image_tag(dockerfile_text: str) -> str:
    digest = hashlib.sha256(dockerfile_text.encode("utf-8")).hexdigest()
    return f"{IMAGE_REPOSITORY}:{digest[:16]}"


def _log(message: str) -> None:
    print(f"bundle_closure: {message}", file=sys.stderr)


def _run(argv: list[str], *, input_text: str | None = None,
         env: Mapping[str, str] | None = None) -> str:
    proc = subprocess.run(argv, input=input_text, capture_output=True, text=True,
                          env=None if env is None else dict(env), check=False)
    if proc.returncode != 0:
        raise CommandError(
            f"{shlex.join(argv)} exited {proc.returncode}:\n{proc.stderr}{proc.stdout}")
    return proc.stdout


def host_resolution(binaries: Mapping[str, Path]) -> dict[str, str]:
    """soname -> host path over every shipped ELF, with LD_LIBRARY_PATH
    removed so only the binaries' own RUNPATHs and the system paths count."""
    env = {k: v for k, v in os.environ.items() if k != "LD_LIBRARY_PATH"}
    resolved: dict[str, str] = {}
    for path in binaries.values():
        report = parse_ldd(_run(["ldd", str(path)], env=env))
        if report.missing or report.version_short:
            raise ClosureError(
                f"{path} does not resolve on the build host itself: "
                + ", ".join(sorted(report.missing | report.version_short)))
        for soname, host_path in report.resolved.items():
            resolved.setdefault(soname, host_path)
    return resolved


def ensure_reference_image(dockerfile_text: str) -> str:
    tag = reference_image_tag(dockerfile_text)
    probe = subprocess.run(["docker", "image", "inspect", tag], capture_output=True,
                           text=True, check=False)
    if probe.returncode != 0:
        _log(f"building reference image {tag}")
        _run(["docker", "build", "-t", tag, "-"], input_text=dockerfile_text)
    return tag


def _copy_writable(source: str | Path, dest: Path, mode: int) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, dest)
    dest.chmod(mode)


def _patch_and_ldd_script(product_name: str) -> str:
    lib_dir = "/probe" + installed_lib_dir(product_name)
    return (
        "set -eu\n"
        f"for f in {shlex.quote(lib_dir)}/*; do\n"
        "  [ -e \"$f\" ] || continue\n"
        f"  patchelf --set-rpath {shlex.quote(LIBRARY_RUNPATH)} \"$f\"\n"
        "done\n"
        "for f in /probe/usr/bin/*; do\n"
        f"  patchelf --set-rpath {shlex.quote(executable_runpath(product_name))} \"$f\"\n"
        "done\n"
        "ldd /probe/usr/bin/*\n")


def docker_probe(image: str, stage: Path, product_name: str,
                 binaries: Mapping[str, Path], host_resolved: Mapping[str, str]) -> Probe:
    """A probe that stages onto `stage` and runs ldd on it in `image`. ldd runs
    with no LD_LIBRARY_PATH, so it checks the as-installed layout."""
    script = _patch_and_ldd_script(product_name)

    def probe(staged: frozenset) -> LddReport:
        for name, path in binaries.items():
            dest = staged_binary(stage, name)
            if not dest.exists():
                _copy_writable(path, dest, 0o755)
        for soname in sorted(staged):
            dest = staged_lib_dir(stage, product_name) / soname
            if not dest.exists():
                _copy_writable(host_resolved[soname], dest, 0o644)
        _log(f"probing {image} with {len(staged)} staged libraries")
        return parse_ldd(_run([
            "docker", "run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
            "-v", f"{stage}:/probe", image, "sh", "-c", script]))

    return probe


def _name_eq_path(text: str) -> tuple[str, Path]:
    name, sep, path = text.partition("=")
    if not sep or not name or not path:
        raise argparse.ArgumentTypeError(f"expected NAME=PATH, got {text!r}")
    return name, Path(path).resolve()


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--tauri-conf", type=Path, required=True)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--overlay", type=Path, required=True)
    parser.add_argument("--main-binary", type=_name_eq_path, required=True)
    parser.add_argument("--extra-binary", type=_name_eq_path, action="append", default=[])
    return parser.parse_args(argv)


def _run_derivation(args: argparse.Namespace) -> None:
    conf = json.loads(args.tauri_conf.read_text(encoding="utf-8"))
    product_name = conf["productName"]
    depends = list(conf["bundle"]["linux"]["deb"]["depends"])
    extras = dict(args.extra_binary)
    binaries = {args.main_binary[0]: args.main_binary[1], **extras}
    stage = args.stage.resolve()
    if stage.exists() and any(stage.iterdir()):
        raise ClosureError(f"stage dir {stage} is not empty; remove it first")
    stage.mkdir(parents=True, exist_ok=True)

    host_resolved = host_resolution(binaries)
    image = ensure_reference_image(reference_dockerfile(depends))
    probe = docker_probe(image, stage, product_name, binaries, host_resolved)
    closure = derive_closure(host_resolved, probe)
    for soname in sorted(closure):
        _log(f"bundled {soname} <- {host_resolved[soname]}")
    _log(f"closure: {len(closure)} libraries; staged at {stage}")

    overlay = tauri_overlay(stage, product_name, args.main_binary, extras)
    args.overlay.parent.mkdir(parents=True, exist_ok=True)
    args.overlay.write_text(json.dumps(overlay, indent=2) + "\n", encoding="utf-8")
    _log(f"wrote tauri overlay {args.overlay}")


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(sys.argv[1:] if argv is None else argv)
    try:
        _run_derivation(args)
    except (ClosureError, CommandError, ValueError, KeyError, OSError) as error:
        _log(f"error: {error}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
