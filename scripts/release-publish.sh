#!/usr/bin/env bash
# make release-publish (spec §13.3, step 2), on main after the bump PR is merged: build dist, create
# the annotated tag vX.Y.Z at HEAD, push it, and create the GitHub release with exactly the tarball
# and SHA256SUMS. A rerun resumes: a tag vX.Y.Z already at HEAD (locally or on origin) is kept, never
# moved or recreated. Every preflight runs before anything changes; DRY_RUN=1 runs them and prints
# the rest.
unset CDPATH # a relative `cd` must not search CDPATH (or print the directory it found)
here=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source-path=SCRIPTDIR source=release-lib.sh
. "$here/release-lib.sh"

parse_dry_run

need_main
need_clean
need_up_to_date
need_gh_auth

version=$(read_version)
need_range "$version"
tag=v$version
head=$(git rev-parse HEAD)

# The version must be above every other release tag, local or on origin.
remote_tags=$(git ls-remote --tags --refs origin 'refs/tags/v*') || die "git ls-remote --tags origin failed"
latest=
while read -r t; do
    t=${t#v}
    if [ "$t" != "$version" ] && is_semver "$t" && { [ -z "$latest" ] || version_gt "$t" "$latest"; }; then
        latest=$t
    fi
done < <({
    git tag --list 'v*'
    printf '%s\n' "$remote_tags" | sed -n 's|^[0-9a-f]*[[:space:]]*refs/tags/||p'
})
if [ -n "$latest" ] && ! version_gt "$version" "$latest"; then
    die "version $version is not above the latest tag v$latest; bump the version with make release-prepare"
fi

# The merged release PR, and its merge commit in this history.
merge=$("$GH" pr list --state merged --base main --head "release/$tag" --json number,mergeCommit \
    --jq '.[0].mergeCommit.oid // empty') || die "gh pr list failed"
[ -n "$merge" ] || die "no merged pull request for release/$tag into main; merge the PR from make release-prepare first"
[[ $merge =~ ^[0-9a-f]{40}$ ]] ||
    die "gh reported a malformed merge commit id for the release/$tag PR (not 40 lowercase hex digits): '$merge'"
git merge-base --is-ancestor "$merge" HEAD 2>/dev/null ||
    die "the merge commit $merge of the release/$tag PR is not an ancestor of HEAD; pull main first"

if "$GH" release view "$tag" >/dev/null 2>&1; then
    die "a GitHub release $tag already exists; nothing to publish (if it looks half-uploaded, inspect it with gh release view $tag, or delete it by hand and rerun)"
fi
need_licenses
need_bench_results "$version"

# Resume state: an existing tag must be annotated and at HEAD, and the same object on both sides.
local_obj=
if local_ref "refs/tags/$tag"; then
    local_obj=$(git rev-parse "refs/tags/$tag")
    [ "$(git cat-file -t "$local_obj")" = tag ] ||
        die "tag $tag exists locally but is lightweight, not annotated; release tags are annotated"
    at=$(git rev-parse "refs/tags/$tag^{commit}")
    [ "$at" = "$head" ] || die "tag $tag exists locally at $at, not at HEAD $head"
fi
remote=$(git ls-remote origin "refs/tags/$tag" "refs/tags/$tag^{}") || die "git ls-remote origin failed"
remote_obj=$(printf '%s\n' "$remote" | awk -v r="refs/tags/$tag" '$2 == r { print $1 }')
remote_at=$(printf '%s\n' "$remote" | awk -v r="refs/tags/$tag^{}" '$2 == r { print $1 }')
if [ -n "$remote_obj" ]; then
    [ -n "$remote_at" ] ||
        die "tag $tag exists on origin but is lightweight, not annotated; release tags are annotated"
    [ "$remote_at" = "$head" ] || die "tag $tag exists on origin at $remote_at, not at HEAD $head"
    if [ -n "$local_obj" ] && [ "$local_obj" != "$remote_obj" ]; then
        die "tag $tag differs between this clone ($local_obj) and origin ($remote_obj)"
    fi
fi

# dist runs before the tag is created, so a failed build leaves no tag behind.
DRY_RUN=$DRY_RUN make --no-print-directory dist || die "make dist failed"
if [ -n "$local_obj" ] || [ -n "$remote_obj" ]; then
    note "resuming: tag $tag already exists at HEAD; it is kept as it is"
else
    run git tag -a "$tag" -m "hali $tag"
fi
if [ -n "$remote_obj" ]; then
    note "resuming: tag $tag is already on origin"
else
    run git push -q origin "refs/tags/$tag"
fi
run "$GH" release create "$tag" --verify-tag --generate-notes \
    "dist/hali-$tag-x86_64-linux.tar.gz" dist/SHA256SUMS
note "published $tag"
