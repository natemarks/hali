# shellcheck shell=bash
# Shared code for scripts/dist.sh, scripts/release-prepare.sh and scripts/release-publish.sh
# (spec §13.3, §13.4). Sourced, not run. The release scripts never delete, move or force tags, and
# never use sudo: a rerun resumes from what already exists.
set -euo pipefail

GH=${GH:-gh}
RELEASE_STATIC=${RELEASE_STATIC:-static-check}
CARGO_EDIT_VERSION=${CARGO_EDIT_VERSION:-0.13.14}
ADR_GLOB='docs/adr/0001-*.md'
BENCH_HEADING='## Benchmark results'

die() {
    printf '%s: %s\n' "${0##*/}" "$*" >&2
    exit 1
}

note() {
    printf '%s: %s\n' "${0##*/}" "$*"
}

# DRY_RUN is empty, 0 or 1; anything else is refused.
parse_dry_run() {
    case "${DRY_RUN-}" in
    "" | 0) DRY_RUN=0 ;;
    1) DRY_RUN=1 ;;
    *) die "DRY_RUN must be empty, 0 or 1, not '${DRY_RUN}'" ;;
    esac
}

# Run a command that changes something; with DRY_RUN=1, print it as `dry-run: <cmd>` instead.
run() {
    if [ "$DRY_RUN" = 1 ]; then
        printf 'dry-run:'
        printf ' %q' "$@"
        printf '\n'
        return 0
    fi
    "$@"
}

is_semver() {
    [[ $1 =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
}

# The [package] version in Cargo.toml.
read_version() {
    local v
    v=$(awk '
        /^\[/ { pkg = ($0 == "[package]") }
        pkg && /^version[[:space:]]*=/ {
            sub(/^version[[:space:]]*=[[:space:]]*"/, ""); sub(/".*$/, ""); print; exit
        }' Cargo.toml)
    is_semver "$v" || die "cannot read a X.Y.Z version from the [package] section of Cargo.toml (got '$v')"
    printf '%s\n' "$v"
}

# Exit status 0 when version $1 is greater than version $2.
version_gt() {
    [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" = "$1" ]
}

need_range() {
    local v=$1 major minor
    major=${v%%.*}
    minor=${v#*.}
    minor=${minor%%.*}
    if [ "$major" != 0 ] || [ "$minor" = 0 ]; then
        die "version $v is outside the release range [0.1.0, 1.0.0): releases stay on 0.x until the spec is fully built"
    fi
}

need_main() {
    local branch
    branch=$(git symbolic-ref -q --short HEAD || echo "a detached HEAD")
    [ "$branch" = main ] || die "releases are cut from main, but this is $branch; switch to main first"
}

need_clean() {
    [ -z "$(git status --porcelain=v1 --untracked-files=normal)" ] ||
        die "the working tree is not clean (uncommitted changes or untracked files); commit or remove them first"
}

need_clean_tracked() {
    [ -z "$(git status --porcelain=v1 --untracked-files=no)" ] ||
        die "the working tree has uncommitted changes to tracked files; dist builds from a clean commit"
}

# Read-only: updates refs/remotes/origin/main only, never local branches or tags.
need_up_to_date() {
    git fetch -q --no-tags origin main || die "git fetch origin main failed; cannot tell whether main is up to date with origin"
    local head remote
    head=$(git rev-parse HEAD)
    remote=$(git rev-parse -q --verify refs/remotes/origin/main) || die "origin has no main branch"
    [ "$head" = "$remote" ] ||
        die "main is not up to date with origin: HEAD is $head, origin/main is $remote (behind or ahead); pull or push first"
}

need_gh_auth() {
    "$GH" auth status >/dev/null 2>&1 || die "gh is not authenticated (gh auth status failed); run: gh auth login"
}

need_cargo_edit() {
    local out
    out=$(cargo set-version --version 2>/dev/null) ||
        die "cargo-edit (cargo set-version) is not installed; run: cargo install cargo-edit --locked --version $CARGO_EDIT_VERSION"
    [ "$out" = "cargo-edit-set-version $CARGO_EDIT_VERSION" ] ||
        die "cargo set-version is '$out', but the release needs cargo-edit $CARGO_EDIT_VERSION; run: cargo install cargo-edit --locked --version $CARGO_EDIT_VERSION"
}

# Both license files are tracked and non-empty at HEAD.
need_licenses() {
    local f size
    for f in LICENSE-MIT LICENSE-APACHE; do
        size=$(git cat-file -s "HEAD:$f" 2>/dev/null) || size=0
        [ "$size" -gt 0 ] || die "$f is missing, untracked or empty at HEAD; the release tarball must include it"
    done
}

# v0.1.0 needs the native benchmark results in ADR-0001 (spec §13.3 checklist).
need_bench_results() {
    local v=$1 adr
    [ "$v" = 0.1.0 ] || return 0
    for adr in $ADR_GLOB; do
        if [ -f "$adr" ] && grep -qx "$BENCH_HEADING" "$adr"; then
            return 0
        fi
    done
    die "v0.1.0 needs a '$BENCH_HEADING' heading in $ADR_GLOB: run make bench natively and record the results first"
}

need_static() {
    make --no-print-directory "$RELEASE_STATIC" || die "make $RELEASE_STATIC failed; fix the static checks first"
}

# Whether a local or origin ref exists.
local_ref() {
    git rev-parse -q --verify "$1" >/dev/null
}

origin_ref() {
    local out
    out=$(git ls-remote origin "$1") || die "git ls-remote origin $1 failed"
    [ -n "$out" ]
}
