#!/usr/bin/env bash
set -uo pipefail
# Criterion 11 (#33): a real `make dist` works on this repository, run in a throwaway local clone
# (never in this worktree). The clone has no remote, `gh` is guarded, and a fixture commit adds the
# license files (and, for 0.1.0, the ADR heading) when this branch does not have them yet, so this
# check judges dist and not the docs. It builds hali in release mode: the first run is slow, the
# build cache in $CACHE is reused afterwards.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1
root=$PWD
CACHE=${HALI_GOAL_DIST_TARGET:-/tmp/hali-goal-dist-target-$(id -u)}

tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
status=0
fail() {
    echo "MISSING: $*"
    status=1
}

git clone -q --no-hardlinks "$root" "$tmp/clone" || { echo "cannot clone $root"; exit 1; }
cd "$tmp/clone" || exit 1
git remote remove origin
export GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0
export GIT_AUTHOR_NAME=goal GIT_AUTHOR_EMAIL=goal@example.invalid
export GIT_COMMITTER_NAME=goal GIT_COMMITTER_EMAIL=goal@example.invalid

# Network guards: a gh that always fails, cargo offline.
mkdir -p "$tmp/guard"
printf '#!/bin/sh\necho "network guard: gh called" >&2\nexit 99\n' >"$tmp/guard/gh"
chmod +x "$tmp/guard/gh"
export PATH="$tmp/guard:$PATH" GH="$tmp/guard/gh" CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$CACHE"

if ! out=$(make -n dist 2>&1); then
    echo "MISSING: make has no usable dist target: $out"
    exit 1
fi

# Fixture commit: licenses and, for 0.1.0, the benchmark heading.
changed=0
[ -s LICENSE-MIT ] || { printf 'MIT fixture\n' >LICENSE-MIT; changed=1; }
[ -s LICENSE-APACHE ] || { printf 'Apache fixture\n' >LICENSE-APACHE; changed=1; }
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ "$version" = "0.1.0" ] && ! grep -q '^## Benchmark results' docs/adr/0001-*.md; then
    printf '\n## Benchmark results\n\nfixture\n' >>docs/adr/0001-rust-turtle-canonical-oxigraph.md
    changed=1
fi
if [ "$changed" = 1 ]; then
    git add -A
    git commit -q -m "fixture: licenses for the dist check" --no-verify || { echo "fixture commit failed"; exit 1; }
fi
[ -z "$(git status --porcelain --untracked-files=no)" ] || { echo "clone is not clean"; exit 1; }

if ! make dist DRY_RUN=0 >"$tmp/dist.out" 2>&1; then
    echo "make dist failed in the clone:"
    tail -n 40 "$tmp/dist.out"
    exit 1
fi

tarball="dist/hali-v$version-x86_64-linux.tar.gz"
[ -f "$tarball" ] || { fail "$tarball was not produced"; exit "$status"; }
[ -f dist/SHA256SUMS ] || fail "dist/SHA256SUMS was not produced"
(cd dist && sha256sum -c SHA256SUMS >/dev/null 2>&1) || fail "sha256sum -c SHA256SUMS fails"
[ "$(find dist -type f | wc -l)" = 2 ] || fail "dist/ must hold exactly the tarball and SHA256SUMS: $(find dist -type f)"

mkdir "$tmp/x"
tar xzf "$tarball" -C "$tmp/x" || { fail "cannot extract $tarball"; exit "$status"; }
top="$tmp/x/hali-v$version"
for f in hali install.sh hali.1 completions/hali.bash completions/_hali completions/hali.fish README.md LICENSE-MIT LICENSE-APACHE; do
    [ -f "$top/$f" ] || fail "tarball lacks hali-v$version/$f"
done
[ -x "$top/hali" ] && [ -x "$top/install.sh" ] || fail "hali and install.sh must be executable"
if [ -x "$top/hali" ]; then
    [ "$(env -i "$top/hali" --version)" = "hali $version" ] || fail "hali --version is not 'hali $version'"
    env -i "$top/hali" generate man | cmp -s - "$top/hali.1" || fail "hali.1 differs from env -i hali generate man"
    for pair in bash:hali.bash zsh:_hali fish:hali.fish; do
        env -i "$top/hali" generate completions "${pair%%:*}" | cmp -s - "$top/completions/${pair#*:}" || fail "completions/${pair#*:} differs from env -i hali generate completions ${pair%%:*}"
    done
    ! grep -aq 'HALI_TEST_' "$top/hali" || fail "the release binary contains HALI_TEST_"
fi
exit "$status"
