#!/usr/bin/env bash
set -uo pipefail
# Criteria 2-15 (#33): the release tests that need outside software (bash, git, tar, shellcheck, jq,
# cargo-edit 0.13.14) are #[ignore]d, so `make goal-tests` skips them; run them here, together with
# the one release test that is not ignored (the scripts never delete or force tags, or use sudo).
# They only ever run against throwaway fixture repos with a fake gh: nothing here tags, pushes or
# contacts GitHub, and the real release targets are never run.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
if ! command -v cargo >/dev/null || ! cargo set-version --version >/dev/null 2>&1; then
    echo "MISSING: cargo-edit (cargo set-version) is not installed; the release tests need 0.13.14: cargo install cargo-edit --locked --version 0.13.14"
    status=1
fi
for tool in git tar jq shellcheck sha256sum gzip; do
    command -v "$tool" >/dev/null || { echo "MISSING: $tool is not installed"; status=1; }
done

echo "--- release tests that need outside software (--ignored)"
cargo test --no-fail-fast --features test-hooks,goal \
    --test release_install --test release_dist --test release_flow \
    -- --ignored || status=1

echo "--- release_scripts (not ignored)"
cargo test --features test-hooks,goal --test release_scripts || status=1

exit "$status"
