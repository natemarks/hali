#!/usr/bin/env bash
set -uo pipefail
# #4 rows 10 and 11: `make install` with HOME (and PREFIX) pointed at a temp dir installs the
# binary, man page and bash/zsh/fish completions at the spec §4 paths with the right modes and
# content, writes nothing else, warns about re-login only when BINDIR was just created, and
# replaces existing files and symlinks without writing through them.
#
# Hermetic: every make runs under `env -i` with only PATH, HOME, CARGO_HOME, RUSTUP_HOME (and
# CARGO_TARGET_DIR when set), so the real home is never read or written; the temp dir is removed
# on exit. The release build goes into the normal target dir, so it is built once (minutes) and
# cached for the later runs and for 05-release.sh.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME RUSTUP_HOME
umask 022

tmp=$(mktemp -d "${TMPDIR:-/tmp}/hali-install.XXXXXX") || exit 1
trap 'chmod -R u+rwX "$tmp" 2>/dev/null; rm -rf "$tmp"' EXIT

fail=0
bad() {
    echo "$*"
    fail=1
}

# inst HOME [make args...]: runs `make install` hermetically; stderr to $tmp/err, stdout to $tmp/out.
inst() {
    local home=$1
    shift
    local keep=()
    [ -n "${CARGO_TARGET_DIR:-}" ] && keep+=("CARGO_TARGET_DIR=$CARGO_TARGET_DIR")
    env -i PATH="$PATH" HOME="$home" CARGO_HOME="$CARGO_HOME" RUSTUP_HOME="$RUSTUP_HOME" \
        "${keep[@]}" make install "$@" >"$tmp/out" 2>"$tmp/err" </dev/null
}

mode() { stat -c %a "$1"; }

# First install into an empty HOME.
home="$tmp/home"
mkdir -p "$home"
if ! inst "$home"; then
    echo "MISSING: \`make install\` failed:"
    tail -8 "$tmp/err"
    exit 1
fi

bin="$home/.local/bin/hali"
man="$home/.local/share/man/man1/hali.1"
bash_c="$home/.local/share/bash-completion/completions/hali"
zsh_c="$home/.local/share/zsh/site-functions/_hali"
fish_c="$home/.config/fish/completions/hali.fish"

for f in "$bin" "$man" "$bash_c" "$zsh_c" "$fish_c"; do
    if [ ! -f "$f" ] || [ -L "$f" ]; then
        bad "MISSING: ${f#"$home"/} is not installed as a regular file"
    fi
done
[ "$(mode "$bin" 2>/dev/null)" = 755 ] || bad "WRONG MODE: ~/.local/bin/hali is not 0755"
for f in "$man" "$bash_c" "$zsh_c" "$fish_c"; do
    [ "$(mode "$f" 2>/dev/null)" = 644 ] || bad "WRONG MODE: ${f#"$home"/} is not 0644"
done

# Content is what `generate` prints.
gen() { env -i "$bin" generate "$@" 2>/dev/null; }
same() {
    if ! cmp -s "$1" <(gen "${@:2}"); then
        bad "WRONG CONTENT: ${1#"$home"/} differs from \`hali generate ${*:2}\`"
    fi
}
if [ -x "$bin" ]; then
    same "$man" man
    same "$bash_c" completions bash
    same "$zsh_c" completions zsh
    same "$fish_c" completions fish
    [ -s "$man" ] || bad "WRONG CONTENT: the man page is empty"
fi

# Nothing else under HOME: exactly these files, in exactly these directories (all 0755).
want_files=$(printf '%s\n' "$bin" "$man" "$bash_c" "$zsh_c" "$fish_c" | sort)
have_files=$(find "$home" ! -type d | sort)
[ "$want_files" = "$have_files" ] || {
    bad "EXTRA: HOME holds files other than the five installed ones:"
    comm -3 <(printf '%s\n' "$want_files") <(printf '%s\n' "$have_files")
}
want_dirs=$(
    for f in "$bin" "$man" "$bash_c" "$zsh_c" "$fish_c"; do
        d=$(dirname "$f")
        while [ "$d" != "$home" ]; do
            echo "$d"
            d=$(dirname "$d")
        done
    done | sort -u
)
have_dirs=$(find "$home" -mindepth 1 -type d | sort)
[ "$want_dirs" = "$have_dirs" ] || {
    bad "EXTRA: HOME holds directories other than the ones the five files need:"
    comm -3 <(printf '%s\n' "$want_dirs") <(printf '%s\n' "$have_dirs")
}
while IFS= read -r d; do
    [ -z "$d" ] || [ "$(mode "$d")" = 755 ] || bad "WRONG MODE: ${d#"$home"/} is not 0755"
done <<<"$have_dirs"

# Row 11: BINDIR was just created, so the re-login warning appears; the zsh note too.
warn="warning: created $home/.local/bin; log out and back in (or start a new login shell) so it is on PATH"
grep -qxF "$warn" "$tmp/err" || {
    bad "MISSING: no line \`$warn\` on stderr of the first install; stderr was:"
    sed 's/^/  /' "$tmp/err"
}
grep -q 'fpath' "$tmp/err" || bad "MISSING: no fpath note when the zsh site-functions directory was created"

# Second run: succeeds, no warning, no fpath note, overwrites edited files, replaces a symlink
# without writing through it.
printf 'stale\n' >"$man"
printf 'stale\n' >"$fish_c"
victim="$tmp/victim"
printf 'precious\n' >"$victim"
rm -f "$bash_c"
ln -s "$victim" "$bash_c"
rm -f "$zsh_c"
ln -s "$victim" "$zsh_c"
if ! inst "$home"; then
    bad "FAILED: the second \`make install\` failed:"
    tail -8 "$tmp/err"
else
    grep -q 'warning: created' "$tmp/err" && bad "FOUND: the second install warns about a created directory"
    grep -q 'fpath' "$tmp/err" && bad "FOUND: the second install repeats the fpath note"
    [ "$(cat "$victim")" = precious ] || bad "WROTE THROUGH: a symlink at the destination was followed"
    [ ! -L "$bash_c" ] && [ ! -L "$zsh_c" ] || bad "SYMLINK KEPT: the symlinks were not replaced by regular files"
    same "$man" man
    same "$fish_c" completions fish
    same "$bash_c" completions bash
    same "$zsh_c" completions zsh
    [ "$(mode "$man" 2>/dev/null)" = 644 ] || bad "WRONG MODE: the replaced man page is not 0644"
fi

# A BINDIR that already exists (off PATH or not): no warning.
home2="$tmp/home2"
mkdir -p "$home2/.local/bin"
if ! inst "$home2"; then
    bad "FAILED: \`make install\` into a HOME whose BINDIR exists failed:"
    tail -8 "$tmp/err"
else
    grep -q 'warning: created' "$tmp/err" && bad "FOUND: a warning although BINDIR already existed"
    [ -x "$home2/.local/bin/hali" ] || bad "MISSING: nothing installed into the existing BINDIR"
fi

# PREFIX moves the binary and the man page; HOME stays untouched.
home3="$tmp/home3"
prefix="$tmp/prefix"
mkdir -p "$home3"
if ! inst "$home3" PREFIX="$prefix"; then
    bad "FAILED: \`make install PREFIX=...\` failed:"
    tail -8 "$tmp/err"
else
    [ -x "$prefix/bin/hali" ] || bad "MISSING: PREFIX did not move the binary to \$PREFIX/bin/hali"
    [ -f "$prefix/share/man/man1/hali.1" ] || bad "MISSING: PREFIX did not move the man page to \$PREFIX/share/man/man1/hali.1"
    [ -e "$home3/.local/bin/hali" ] && bad "FOUND: PREFIX was ignored: the binary went under HOME"
fi

# Row 16: every directory the install creates is 0755 whatever the caller's umask; files are
# 0755 (the binary) or 0644.
audit() {
    local h=$1 label=$2 d f
    while IFS= read -r d; do
        [ -z "$d" ] || [ "$(mode "$d")" = 755 ] || bad "WRONG MODE ($label): ${d#"$h"/} is $(mode "$d"), not 0755"
    done < <(find "$h" -mindepth 1 -type d | sort)
    while IFS= read -r f; do
        [ -z "$f" ] && continue
        if [ "$f" = "$h/.local/bin/hali" ]; then
            [ "$(mode "$f")" = 755 ] || bad "WRONG MODE ($label): ${f#"$h"/} is $(mode "$f"), not 0755"
        else
            [ "$(mode "$f")" = 644 ] || bad "WRONG MODE ($label): ${f#"$h"/} is $(mode "$f"), not 0644"
        fi
    done < <(find "$h" ! -type d | sort)
}
for um in 000 002; do
    uhome="$tmp/home-umask$um"
    mkdir -p "$uhome"
    if ! (umask "$um" && inst "$uhome"); then
        bad "FAILED: \`make install\` under umask $um failed:"
        tail -8 "$tmp/err"
    else
        audit "$uhome" "umask $um"
    fi
done

# Row 17: make variables are data, not shell text: nothing in them is executed, and the files
# land at the literal path (or the target fails with nothing executed).
n=0
while IFS= read -r payload; do
    n=$((n + 1))
    cdir="$tmp/inj$n"
    mkdir -p "$cdir/home"
    mark="$cdir/pwned"
    arg=${payload//@MARK@/$mark}
    lit=${arg//\$\$/\$}
    if inst "$cdir/home" BINDIR="$cdir/$arg"; then
        [ -x "$cdir/$lit/hali" ] || bad "MISSING (injection case $n): BINDIR=$arg did not land at the literal path $cdir/$lit/hali"
    fi
    [ ! -e "$mark" ] || bad "EXECUTED (injection case $n): BINDIR=$arg ran shell text from the make variable"
done <<'EOF'
b";touch @MARK@;echo "
b$$(touch @MARK@)
b`touch @MARK@`
b;touch @MARK@;
b $$HOME 'x' ;touch @MARK@
EOF

# Row 18: a failure part way through leaves no staged temp file behind.
home5="$tmp/home5"
mkdir -p "$home5/.local/share/man/man1/hali.1/keep"
if inst "$home5"; then
    bad "FAILED TO FAIL: \`make install\` succeeded although <MANDIR>/hali.1 is a non-empty directory"
fi
left=$(find "$home5" -name '*.tmp.*')
[ -z "$left" ] || {
    bad "LEFT BEHIND: a failed install left staged temp files:"
    printf '%s\n' "$left"
}

exit "$fail"
