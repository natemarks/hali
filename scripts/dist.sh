#!/usr/bin/env bash
# make dist (spec §13.4): build the release artifacts for x86_64-unknown-linux-gnu from a clean
# commit into dist/: hali-vX.Y.Z-x86_64-linux.tar.gz (reproducible: sorted, owner 0, mtime = the
# commit time, gzip -n) and SHA256SUMS. Needs bash, git, cargo, jq, tar, gzip and sha256sum.
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
# shellcheck source-path=SCRIPTDIR source=release-lib.sh
. "$here/release-lib.sh"

target=x86_64-unknown-linux-gnu

parse_dry_run
need_clean_tracked
need_licenses
version=$(read_version)
need_bench_results "$version"
command -v jq >/dev/null || die "jq is not installed; dist reads cargo's JSON output with jq"

name=hali-v$version-x86_64-linux.tar.gz
build=(cargo build --release --locked --target "$target" --message-format=json-render-diagnostics)

if [ "$DRY_RUN" = 1 ]; then
    run "${build[@]}"
    run tar --sort=name --owner=0 --group=0 --numeric-owner --format=gnu -c "hali-v$version"
    run gzip -n -9
    run mv -f -T "$name" "dist/$name"
    run sha256sum "$name"
    run mv -f -T SHA256SUMS dist/SHA256SUMS
    exit 0
fi

# A release build without test hooks or outside compiler flags (spec §14.2).
json=$(env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER \
    -u CARGO_BUILD_RUSTFLAGS -u CARGO_BUILD_RUSTC_WRAPPER -u CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS \
    "${build[@]}") || die "cargo build --release failed"
bin=$(printf '%s\n' "$json" | jq -r 'select(.reason == "compiler-artifact" and .target.name == "hali"
    and (.target.kind | index("bin")) and .executable != null) | .executable' | tail -n 1)
if [ -z "$bin" ] || [ ! -x "$bin" ]; then
    die "cargo did not report a hali binary"
fi
if grep -aq 'HALI_TEST_' "$bin"; then
    die "$bin contains HALI_TEST_ (test hooks are compiled in); refusing to package it"
fi
got=$(env -i "$bin" --version) || die "$bin --version failed"
[ "$got" = "hali $version" ] || die "$bin --version prints '$got', not 'hali $version'"

epoch=$(git log -1 --format=%ct HEAD)
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
top=$stage/hali-v$version
mkdir -p -- "$top/completions"
install -m 0755 -- "$bin" "$top/hali"
install -m 0755 -- scripts/install.sh "$top/install.sh"
for f in README.md LICENSE-MIT LICENSE-APACHE; do
    install -m 0644 -- "$f" "$top/$f"
done
env -i "$bin" generate man >"$top/hali.1"
env -i "$bin" generate completions bash >"$top/completions/hali.bash"
env -i "$bin" generate completions zsh >"$top/completions/_hali"
env -i "$bin" generate completions fish >"$top/completions/hali.fish"
chmod 0644 -- "$top/hali.1" "$top/completions/hali.bash" "$top/completions/_hali" "$top/completions/hali.fish"
chmod 0755 -- "$top" "$top/completions"

tar -C "$stage" --sort=name --owner=0 --group=0 --numeric-owner --format=gnu \
    --mtime="@$epoch" -cf - "hali-v$version" | gzip -n -9 >"$stage/$name"
(cd -- "$stage" && sha256sum -- "$name") >"$stage/SHA256SUMS"

mkdir -p dist
mv -f -T -- "$stage/$name" "dist/$name"
mv -f -T -- "$stage/SHA256SUMS" dist/SHA256SUMS
note "built dist/$name and dist/SHA256SUMS"
