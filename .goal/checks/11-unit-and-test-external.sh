#!/usr/bin/env bash
# shellcheck disable=SC2016
set -uo pipefail
# Criterion P1 (#10): `make unit` is self-contained and builds with test-hooks; the new
# `make test-external` runs the #[ignore]d tests; AGENTS.md and spec §13.2/§14.7 say so.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

missing=0
need() {
    echo "MISSING: $*"
    missing=1
}

# --- Makefile -------------------------------------------------------------------------------
unit_recipe="$(make -n unit 2>/dev/null)"
case "$unit_recipe" in
    *"cargo test"*"--features test-hooks"*) ;;
    *) need "'make unit' does not run 'cargo test --features test-hooks' (got: ${unit_recipe:-nothing})" ;;
esac
case "$unit_recipe" in
    *"--ignored"* | *"--include-ignored"*) need "'make unit' must not run the #[ignore]d tests" ;;
esac

ext_recipe="$(make -n test-external 2>/dev/null)"
case "$ext_recipe" in
    *"cargo test --features test-hooks -- --ignored"*) ;;
    *) need "'make test-external' does not run 'cargo test --features test-hooks -- --ignored' (got: ${ext_recipe:-no such target})" ;;
esac
if ! grep -E '^\.PHONY:' Makefile | grep -qw 'test-external'; then
    need "test-external is not in a .PHONY line in Makefile"
fi
if ! grep -Eq '^test-external:.*##' Makefile; then
    need "Makefile: 'test-external:' has no '##' help text"
fi
# static and static-check keep running the self-contained unit target, not test-external.
for target in static static-check; do
    line="$(grep -E "^$target:" Makefile)"
    case "$line" in
        *test-external*) need "'$target' must not depend on test-external (it needs shared-mime-info)" ;;
    esac
    case "$line" in
        *" unit "*) ;;
        *) need "'$target' no longer depends on unit" ;;
    esac
done

# --- AGENTS.md (CLAUDE.md imports it) ----------------------------------------------------------
if ! grep -Eq '^\|[[:space:]]*`make test-external`' AGENTS.md; then
    need "AGENTS.md: the Commands table has no row for \`make test-external\`"
fi
unit_row="$(grep -E '^\|[[:space:]]*`make unit`' AGENTS.md)"
case "$unit_row" in
    *test-hooks*) ;;
    *) need "AGENTS.md: the \`make unit\` row does not say it runs with test-hooks" ;;
esac
if ! grep -Eiq '#\[ignore' AGENTS.md; then
    need "AGENTS.md: no Rules bullet says tests needing outside software are #[ignore]d"
fi
if ! grep -Eiq '^- .*(outside|external).*#\[ignore|^- .*#\[ignore.*(outside|external)' AGENTS.md; then
    need "AGENTS.md: the rule must be a bullet naming outside software and #[ignore]"
fi

# --- spec §13.2 and §14.7 ----------------------------------------------------------------------
if ! grep -Eq '^\|[[:space:]]*`test-external`' docs/spec.md; then
    need "docs/spec.md §13.2: the Make targets table has no \`test-external\` row"
fi
unit_spec="$(grep -E '^\|[[:space:]]*`unit`' docs/spec.md)"
case "$unit_spec" in
    *test-hooks*) ;;
    *) need "docs/spec.md §13.2: the \`unit\` row does not say it runs with test-hooks" ;;
esac
sec147="$(awk '/^### 14\.7/{f=1;next} /^(### |## )/{f=0} f' docs/spec.md)"
case "$sec147" in
    *"#[ignore"*"test-external"* | *"test-external"*"#[ignore"*) ;;
    *) need "docs/spec.md §14.7 must say MIME-database tests are #[ignore]d and run by make test-external" ;;
esac
case "$sec147" in
    *HALI_SKIP_MIME_TESTS*) need "docs/spec.md §14.7 must not mention HALI_SKIP_MIME_TESTS" ;;
esac

# --- agent and skill files must not still call `make unit` plain `cargo test` -------------------
stale="$(grep -rnE 'make unit[^.]*cargo test([^ ]|$)|`unit`[^.]*`cargo test`' .claude/agents .claude/skills 2>/dev/null)"
if [ -n "$stale" ]; then
    need ".claude agents/skills still describe make unit as plain cargo test:
$stale"
fi

exit "$missing"
