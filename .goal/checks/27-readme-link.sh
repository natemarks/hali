#!/usr/bin/env bash
set -euo pipefail

# Row 18 (#27): the README links the agent usage guide.
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

if ! grep -Fq '](docs/agent-usage.md)' README.md; then
    echo "27-readme-link: README.md has no markdown link to docs/agent-usage.md (expected '](docs/agent-usage.md)')" >&2
    exit 1
fi
echo "27-readme-link: ok"
