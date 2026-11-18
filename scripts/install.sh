#!/bin/sh
# install.sh: installs hali from an unpacked release tarball (spec §13.4, §13.5) into the user-wide
# locations of spec §4. It reads only HOME and PATH, writes only the five files below (and their
# directories), installs each file atomically (a temp file in the destination directory, then
# `mv -f -T`), replaces a symlink without writing through it, and never needs root.
set -eu

prog=install.sh
die() {
    printf '%s: error: %s\n' "$prog" "$*" >&2
    exit 1
}
note() {
    printf '%s: %s\n' "$prog" "$*" >&2
}

case "${HOME-}" in
*[[:cntrl:]]*) die "HOME contains a newline or another control character; refusing to install" ;;
/*) ;;
*) die "HOME must be an absolute path, but it is '${HOME-}'" ;;
esac

unset CDPATH
here=$(cd -P -- "$(dirname -- "$0")" && pwd -P)
umask 022

bin_dir=$HOME/.local/bin
man_dir=$HOME/.local/share/man/man1
bash_dir=$HOME/.local/share/bash-completion/completions
zsh_dir=$HOME/.local/share/zsh/site-functions
fish_dir=$HOME/.config/fish/completions

# mode, source (relative to this directory), destination
files="0755 hali $bin_dir/hali
0644 hali.1 $man_dir/hali.1
0644 completions/hali.bash $bash_dir/hali
0644 completions/_hali $zsh_dir/_hali
0644 completions/hali.fish $fish_dir/hali.fish"

# Every existing component of a destination's directory chain must be a directory (or a symlink to
# one), so that `mkdir -p` cannot fail halfway through the install.
need_dirs() { # <directory>
    d=$1
    while [ "$d" != / ] && [ ! -d "$d" ]; do
        if [ -e "$d" ] || [ -L "$d" ]; then
            die "$d is not a directory; move it aside and run install.sh again"
        fi
        d=$(dirname -- "$d")
    done
}

# Check everything before writing anything.
while read -r mode src dest; do
    [ -f "$here/$src" ] || die "$here/$src is missing; run install.sh from the unpacked release tarball"
    if [ -d "$dest" ] && [ ! -L "$dest" ]; then
        die "$dest is a directory; remove it and run install.sh again"
    fi
    need_dirs "$(dirname -- "$dest")"
done <<EOF
$files
EOF

bin_new=false
[ -d "$bin_dir" ] || bin_new=true
zsh_new=false
[ -d "$zsh_dir" ] || zsh_new=true

cur=
trap 'if [ -n "$cur" ]; then rm -f -- "$cur"; fi' EXIT
trap 'exit 1' HUP INT TERM

put() { # <mode> <source> <destination>
    dir=$(dirname -- "$3")
    mkdir -p -- "$dir"
    cur=$(mktemp "$dir/.hali-install.XXXXXXXX")
    cat -- "$2" >"$cur"
    chmod "$1" -- "$cur"
    mv -f -T -- "$cur" "$3"
    cur=
}

while read -r mode src dest; do
    put "$mode" "$here/$src" "$dest"
done <<EOF
$files
EOF

note "installed hali to ~/.local/bin/hali, the man page and the bash, zsh and fish completions"
if [ "$bin_new" = true ]; then
    note "warning: created ~/.local/bin; log out and back in (or start a new login shell) so it is on PATH"
fi
if [ "$zsh_new" = true ]; then
    note "note: add ~/.local/share/zsh/site-functions to fpath before compinit in ~/.zshrc for zsh completions"
fi

if [ ! -d "$HOME/.claude" ]; then
    note "no ~/.claude directory: skipped the Claude Code skill (run 'hali skill install' later if you use it)"
elif ! "$bin_dir/hali" skill --help >/dev/null 2>&1; then
    note "this hali has no 'skill' command: skipped the Claude Code skill"
else
    "$bin_dir/hali" skill install || die "hali skill install failed"
fi
