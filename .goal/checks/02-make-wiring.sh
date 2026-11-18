#!/usr/bin/env bash
set -uo pipefail
# Criteria 1 and 14 (#33): the Makefile wiring and the shellcheck target.
#   - `make help` lists dist, release-prepare and release-publish; all three are .PHONY.
#   - Each target runs its own script and passes BUMP, DRY_RUN, GH, RELEASE_STATIC and
#     CARGO_EDIT_VERSION. Checked against a copy of the Makefile with stub scripts that only print
#     their environment: the real scripts are never run.
#   - `make shellcheck` covers scripts/install.sh at default severity (an info-level finding added
#     to a copy of install.sh must fail it), and the real install.sh passes.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1
root=$PWD
status=0
fail() {
    echo "MISSING: $*"
    status=1
}

help=$(make --no-print-directory help 2>&1)
for t in dist release-prepare release-publish; do
    printf '%s\n' "$help" | grep -Eq "^${t}[: ]" || fail "make help does not list $t"
done
phony=$(make --no-print-directory -pn 2>/dev/null | grep -E '^\.PHONY:' || true)
for t in dist release-prepare release-publish; do
    printf '%s\n' "$phony" | grep -Eq "(^|[[:space:]])${t}([[:space:]]|$)" || fail ".PHONY does not list $t"
done

tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT

# --- wiring: stub scripts print what they received
mkdir -p "$tmp/wire/scripts"
cp Makefile "$tmp/wire/Makefile"
for s in dist release-prepare release-publish; do
    cat >"$tmp/wire/scripts/$s.sh" <<EOF
#!/usr/bin/env bash
echo "STUB $s BUMP=\${BUMP-<unset>} DRY_RUN=\${DRY_RUN-<unset>} GH=\${GH-<unset>} RELEASE_STATIC=\${RELEASE_STATIC-<unset>} CARGO_EDIT_VERSION=\${CARGO_EDIT_VERSION-<unset>}"
EOF
    chmod +x "$tmp/wire/scripts/$s.sh"
done
want() { # <output> <target> <fragment>
    case "$1" in *"$3"*) ;; *) fail "make $2: expected '$3' in: $1" ;; esac
}
for t in dist release-prepare release-publish; do
    out=$(cd "$tmp/wire" && make --no-print-directory "$t" BUMP=minor DRY_RUN=1 GH=/x/gh RELEASE_STATIC=my-static 2>&1)
    want "$out" "$t" "STUB $t "
    want "$out" "$t" "BUMP=minor"
    want "$out" "$t" "DRY_RUN=1"
    want "$out" "$t" "GH=/x/gh"
    want "$out" "$t" "RELEASE_STATIC=my-static"
    want "$out" "$t" "CARGO_EDIT_VERSION=0.13.14"
    out=$(cd "$tmp/wire" && env -u BUMP -u DRY_RUN -u GH -u RELEASE_STATIC make --no-print-directory "$t" 2>&1)
    want "$out" "$t (defaults)" "GH=gh"
    want "$out" "$t (defaults)" "RELEASE_STATIC=static-check"
    want "$out" "$t (defaults)" "CARGO_EDIT_VERSION=0.13.14"
    out=$(cd "$tmp/wire" && env BUMP=patch make --no-print-directory "$t" 2>&1)
    want "$out" "$t (BUMP from the environment)" "BUMP=patch"
done

# --- quoting (security finding 5): a value with a quote must reach the script as data, never run as
# a command. Each variable in turn gets "x'; touch <marker>; '" on the make command line; the marker
# must not appear and the stub must print the value unchanged.
for t in dist release-prepare release-publish; do
    for var in BUMP DRY_RUN GH RELEASE_STATIC; do
        marker="$tmp/pwned-$t-$var"
        val="x'; touch $marker; '"
        out=$(cd "$tmp/wire" && make --no-print-directory "$t" "$var=$val" 2>&1)
        [ ! -e "$marker" ] || fail "make $t: $var=\"...'; touch ...; '\" was run as a shell command (the Makefile splices values into the command line; export them instead)"
        case "$out" in *"$var=$val "*) ;; *) fail "make $t: $var did not reach the script unchanged; got: $out" ;; esac
    done
done

# --- shellcheck target covers install.sh at default severity
if [ ! -f scripts/install.sh ]; then
    fail "scripts/install.sh does not exist"
else
    git ls-files --error-unmatch scripts/install.sh >/dev/null 2>&1 || fail "scripts/install.sh is not tracked by git (make shellcheck uses git ls-files)"
    [ "$(head -n 1 scripts/install.sh)" = "#!/bin/sh" ] || fail "scripts/install.sh must start with #!/bin/sh"
    shellcheck scripts/install.sh || fail "scripts/install.sh is not shellcheck-clean at default severity"

    mkdir -p "$tmp/sc/scripts"
    cp Makefile "$tmp/sc/Makefile"
    cp scripts/install.sh "$tmp/sc/scripts/install.sh"
    (cd "$tmp/sc" && git init -q && git add -A) >/dev/null 2>&1
    (cd "$tmp/sc" && make --no-print-directory shellcheck >"$tmp/sc.out" 2>&1) || fail "make shellcheck fails on the real install.sh: $(cat "$tmp/sc.out")"
    # An info-level finding (SC2086, unquoted variable) must fail the target.
    # shellcheck disable=SC2016
    printf '\nx=$(echo hi)\necho $x\n' >>"$tmp/sc/scripts/install.sh"
    (cd "$tmp/sc" && git add -A) >/dev/null 2>&1
    if (cd "$tmp/sc" && make --no-print-directory shellcheck >/dev/null 2>&1); then
        fail "make shellcheck does not run scripts/install.sh at default severity (an SC2086 finding passed)"
    fi
fi
cd "$root" || exit 1
exit "$status"
