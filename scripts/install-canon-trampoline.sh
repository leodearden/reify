#!/usr/bin/env bash
# Install the canonical-build-path cargo trampoline (scripts/canon-bin/cargo)
# for this user's interactive sessions; called by setup-dev.sh, idempotent.
#
#   - a COPY at ~/.local/share/reify-canon/bin/cargo, never a symlink: setup-dev
#     runs from warm lanes, and a link into a reclaimed lane dangles (task 5888);
#   - login shells: a marked block re-placed LAST in ~/.profile, so it follows
#     ~/.local/bin (rustup's cargo symlink) and `. ~/.cargo/env`;
#   - systemd --user units, which T3-hosted sessions inherit: an environment.d
#     drop-in sorting after the system files and 995-user-path.conf.
#
# ~/.bash_profile and ~/.bash_login are user-owned and never edited; the shadow
# report names them when they put another cargo first. Shadowing warns and
# exits 0: a bypassed trampoline builds at the real path, never wrongly.
# Why the trampoline exists: CLAUDE.md "Canonical build path".
#
# Usage: scripts/install-canon-trampoline.sh   (acts on $HOME only)
set -euo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*"; }

SOURCE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/canon-bin/cargo"
MARKER="reify-canon-trampoline"
BIN_DIR="$HOME/.local/share/reify-canon/bin"
INSTALLED="$BIN_DIR/cargo"
PROFILE="$HOME/.profile"
BLOCK_START="# >>> reify-canon trampoline (scripts/install-canon-trampoline.sh) >>>"
BLOCK_END="# <<< reify-canon trampoline <<<"
DROP_IN="${XDG_CONFIG_HOME:-$HOME/.config}/environment.d/999-reify-canon-trampoline.conf"
ENV_GENERATOR="/usr/lib/systemd/user-environment-generators/30-systemd-environment-d-generator"

PENDING=""
trap 'rm -f "${PENDING:-}"' EXIT

# pending_in <dir>: a fresh temp file in dir, so the final mv is a rename.
pending_in() {
    mkdir -p "$1" || { err "cannot create $1"; exit 1; }
    PENDING="$(mktemp "$1/.reify-canon.XXXXXX")" || { err "cannot write in $1"; exit 1; }
}

# commit_pending <dest> <mode>: returns 0 when dest was (re)written from
# $PENDING, 1 when dest already is a regular file with those bytes.
commit_pending() {
    local dest="$1" mode="$2"
    if [ -f "$dest" ] && [ ! -L "$dest" ] && cmp -s "$PENDING" "$dest"; then
        rm -f "$PENDING"
        PENDING=""
        return 1
    fi
    chmod "$mode" "$PENDING" && mv -fT "$PENDING" "$dest" || { err "cannot write $dest"; exit 1; }
    PENDING=""
}

report_commit() {
    if commit_pending "$1" "$2"; then ok "$3: written ($1)"; else ok "$3: up to date ($1)"; fi
}

# ---------- source ----------
head_bytes=""
[ -f "$SOURCE" ] && head_bytes="$(head -c 1024 "$SOURCE")"
if [[ "$head_bytes" != *"$MARKER"* ]]; then
    err "$SOURCE is missing or lacks the $MARKER marker; refusing to install it"
    exit 1
fi

# ---------- the installed copy ----------
pending_in "$BIN_DIR"
cp "$SOURCE" "$PENDING" || { err "cannot copy $SOURCE"; exit 1; }
report_commit "$INSTALLED" 0755 "cargo trampoline"

# ---------- login shells: ~/.profile ----------
profile_target="$PROFILE"
[ -L "$PROFILE" ] && profile_target="$(readlink -f "$PROFILE")"
profile_mode=644
[ -f "$profile_target" ] && profile_mode="$(stat -c %a "$profile_target")"
pending_in "$(dirname "$profile_target")"
{
    if [ -f "$profile_target" ]; then
        # Drops each complete marked block (an unterminated one is kept) and
        # trailing blank lines, so re-placing the block is idempotent.
        awk -v start="$BLOCK_START" -v end="$BLOCK_END" '
            $0 == start { inblock = 1; held = 0 }
            inblock { hold[++held] = $0; if ($0 == end) inblock = 0; next }
            { line[++n] = $0 }
            END {
                if (inblock) for (i = 1; i <= held; i++) line[++n] = hold[i]
                while (n > 0 && line[n] ~ /^[[:space:]]*$/) n--
                for (i = 1; i <= n; i++) print line[i]
                if (n > 0) print ""
            }' "$profile_target"
    fi
    printf '%s\n' "$BLOCK_START"
    printf '%s\n' 'case "$PATH" in'
    printf '%s\n' '    "$HOME/.local/share/reify-canon/bin" | "$HOME/.local/share/reify-canon/bin":*) ;;'
    printf '%s\n' '    *) PATH="$HOME/.local/share/reify-canon/bin:$PATH" ;;'
    printf '%s\n' 'esac'
    printf '%s\n' 'export PATH'
    printf '%s\n' "$BLOCK_END"
} > "$PENDING" || { err "cannot render $PROFILE"; exit 1; }
report_commit "$profile_target" "$profile_mode" "login-shell PATH block"

# ---------- systemd --user units: environment.d ----------
pending_in "$(dirname "$DROP_IN")"
printf 'PATH=%s:${PATH}\n' "$BIN_DIR" > "$PENDING" || { err "cannot render $DROP_IN"; exit 1; }
report_commit "$DROP_IN" 0644 "systemd --user PATH drop-in"

# ---------- shadow report ----------
first_cargo_on() {
    local entry
    local -a entries
    IFS=: read -r -a entries <<<"$1"
    for entry in "${entries[@]}"; do
        if [ -n "$entry" ] && [ -f "$entry/cargo" ] && [ -x "$entry/cargo" ]; then
            printf '%s\n' "$entry/cargo"
            return 0
        fi
    done
}

login_out="$(env -i HOME="$HOME" bash -lc 'command -v cargo' 2>/dev/null)" || login_out=""
login_cargo="${login_out##*$'\n'}"
if [ "$login_cargo" = "$INSTALLED" ]; then
    ok "login shells resolve cargo to the trampoline"
else
    warn "login shells resolve cargo to ${login_cargo:-nothing}, not $INSTALLED"
    for startup in "$HOME/.bash_profile" "$HOME/.bash_login"; do
        if [ -e "$startup" ]; then
            warn "  bash login shells read $startup instead of ~/.profile: put $BIN_DIR first on PATH there"
            break
        fi
    done
fi

if [ -x "$ENV_GENERATOR" ]; then
    generator_env=(env -i HOME="$HOME")
    [ -n "${XDG_CONFIG_HOME:-}" ] && generator_env+=(XDG_CONFIG_HOME="$XDG_CONFIG_HOME")
    generated="$("${generator_env[@]}" "$ENV_GENERATOR" 2>/dev/null)" || generated=""
    unit_path=""
    while IFS= read -r line; do
        if [[ "$line" == PATH=* ]]; then unit_path="${line#PATH=}"; fi
    done <<<"$generated"
    unit_cargo="$(first_cargo_on "$unit_path")"
    if [ "$unit_cargo" = "$INSTALLED" ]; then
        ok "systemd --user units resolve cargo to the trampoline (from the user manager's next start)"
    else
        warn "systemd --user units resolve cargo to ${unit_cargo:-nothing}, not $INSTALLED"
    fi
fi
