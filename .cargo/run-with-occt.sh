#!/bin/bash
# The cargo target runner (.cargo/config.toml), also honoured by nextest for
# every test process: prepends the cargo-target loader dirs, then execs. Why the
# deps dir is still load-bearing although RUNPATH is baked into most binaries:
# docs/notes/native-dep-loader-path.md
case "${BASH_SOURCE[0]}" in
    */*) _occt_runner_dir="${BASH_SOURCE[0]%/*}" ;;
    *) _occt_runner_dir=. ;;
esac
# shellcheck source=scripts/lib_native_ld_path.sh
source "$_occt_runner_dir/../scripts/lib_native_ld_path.sh" || {
    echo "run-with-occt.sh: cannot source $_occt_runner_dir/../scripts/lib_native_ld_path.sh" >&2
    exit 1
}
native_ld_path_prepend cargo-target || exit 1
exec "$@"
