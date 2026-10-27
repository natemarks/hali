#!/usr/bin/env bash
set -uo pipefail
# Criterion 2 (#10): CI installs shared-mime-info and runs `make test-external` as a separate
# job, so the #[ignore]d MIME-database tests run on every PR and push.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

shopt -s nullglob
found=""
for wf in .github/workflows/*.yml .github/workflows/*.yaml; do
    if grep -Eq 'make[[:space:]]+test-external' "$wf"; then
        found="$wf"
        break
    fi
done
if [ -z "$found" ]; then
    echo "MISSING: no workflow under .github/workflows runs 'make test-external'"
    exit 1
fi

missing=0
if ! grep -q 'shared-mime-info' "$found"; then
    echo "MISSING: $found runs 'make test-external' but does not install shared-mime-info"
    missing=1
fi
# A separate job: the job that runs test-external must not be the one that runs static-check.
if awk '
    /^  [A-Za-z0-9_-]+:[[:space:]]*$/ { job = $1 }
    /make[[:space:]]+test-external/ { ext = job }
    /make[[:space:]]+static-check/ { stat = job }
    END { exit !(ext != "" && stat != "" && ext == stat) }
' "$found"; then
    echo "MISSING: 'make test-external' must be its own job, not a step of the static-check job"
    missing=1
fi
exit "$missing"
