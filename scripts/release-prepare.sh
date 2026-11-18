#!/usr/bin/env bash
# make release-prepare BUMP=patch|minor|major (spec §13.3, step 1): from an up-to-date, clean main,
# open the version-bump PR: branch release/vX.Y.Z, `cargo set-version --bump`, one commit touching
# only Cargo.toml and Cargo.lock, push the branch, `gh pr create`, back to main. Every preflight
# runs before anything changes; DRY_RUN=1 runs them and prints the rest.
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source-path=SCRIPTDIR source=release-lib.sh
. "$here/release-lib.sh"

case "${BUMP-}" in
patch | minor | major) ;;
*) die "BUMP must be patch, minor or major, not '${BUMP-}' (make release-prepare BUMP=patch)" ;;
esac
parse_dry_run

need_main
need_clean
need_up_to_date
need_gh_auth
need_cargo_edit

current=$(read_version)
IFS=. read -r major minor patch <<<"$current"
case "$BUMP" in
patch) new=$major.$minor.$((patch + 1)) ;;
minor) new=$major.$((minor + 1)).0 ;;
major) new=$((major + 1)).0.0 ;;
esac
need_range "$new"

tag=v$new
branch=release/$tag
if local_ref "refs/tags/$tag"; then die "tag $tag already exists locally"; fi
if origin_ref "refs/tags/$tag"; then die "tag $tag already exists on origin"; fi
if local_ref "refs/heads/$branch"; then die "branch $branch already exists locally"; fi
if origin_ref "refs/heads/$branch"; then die "branch $branch already exists on origin"; fi
need_licenses
need_bench_results "$new"
need_static

note "preparing $tag (from $current, BUMP=$BUMP)"
created=0
on_exit() {
    local code=$?
    if [ "$code" != 0 ] && [ "$created" = 1 ]; then
        printf '%s: stopped on branch %s; inspect it, then switch back with: git checkout main\n' "${0##*/}" "$branch" >&2
    fi
}
trap on_exit EXIT

run git checkout -q -b "$branch"
created=1
run cargo set-version --bump "$BUMP"
if [ "$DRY_RUN" = 0 ]; then
    changed=$(git status --porcelain=v1 --untracked-files=normal | cut -c4- | sort | tr '\n' ' ')
    [ "$changed" = "Cargo.lock Cargo.toml " ] ||
        die "cargo set-version changed '$changed', expected exactly Cargo.lock and Cargo.toml"
    [ "$(read_version)" = "$new" ] || die "cargo set-version did not set the version to $new"
    cargo metadata --locked --offline --format-version 1 >/dev/null ||
        die "Cargo.lock is not up to date after the bump (cargo metadata --locked --offline failed)"
fi
run git add -- Cargo.toml Cargo.lock
run git commit -q -m "release: $tag"
run git push -q origin "refs/heads/$branch"
run "$GH" pr create --base main --head "$branch" --title "release: $tag" \
    --body "Bumps the version to $new. After merging, run make release-publish on main."
run git checkout -q main
created=0
note "opened the release PR for $tag; merge it, then run make release-publish on main"
